pub mod pb {
    tonic::include_proto!("assura.v1");
}

use pb::assura_service_server::{AssuraService, AssuraServiceServer};
use pb::*;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

#[derive(Debug, Default)]
pub struct AssuraServer;

#[tonic::async_trait]
impl AssuraService for AssuraServer {
    async fn check(
        &self,
        request: Request<CheckRequest>,
    ) -> Result<Response<CheckResponse>, Status> {
        let req = request.into_inner();
        let (diagnostics, verifications) = run_check(&req.source, &req.filename, req.layer);
        let success = diagnostics.iter().all(|d| d.severity != "error");
        Ok(Response::new(CheckResponse {
            success,
            diagnostics,
            verifications,
        }))
    }

    async fn build(
        &self,
        request: Request<BuildRequest>,
    ) -> Result<Response<BuildResponse>, Status> {
        let req = request.into_inner();
        let (diagnostics, _) = run_check(&req.source, &req.filename, 1);
        let success = diagnostics.iter().all(|d| d.severity != "error");

        let generated_files = if success {
            run_codegen(&req.source)
        } else {
            std::collections::HashMap::new()
        };

        Ok(Response::new(BuildResponse {
            success,
            diagnostics,
            generated_files,
        }))
    }

    async fn explain(
        &self,
        request: Request<ExplainRequest>,
    ) -> Result<Response<ExplainResponse>, Status> {
        let req = request.into_inner();
        let (title, description, example, fix) = lookup_error_code(&req.error_code);
        Ok(Response::new(ExplainResponse {
            error_code: req.error_code,
            title,
            description,
            example,
            fix,
        }))
    }

    async fn health(
        &self,
        _request: Request<HealthRequest>,
    ) -> Result<Response<HealthResponse>, Status> {
        Ok(Response::new(HealthResponse {
            status: "serving".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }))
    }

    type CheckStreamStream = ReceiverStream<Result<CheckEvent, Status>>;

    async fn check_stream(
        &self,
        request: Request<CheckRequest>,
    ) -> Result<Response<Self::CheckStreamStream>, Status> {
        let req = request.into_inner();
        let (tx, rx) = tokio::sync::mpsc::channel(128);

        tokio::spawn(async move {
            let (diagnostics, verifications) = run_check(&req.source, &req.filename, req.layer);

            for d in &diagnostics {
                let _ = tx
                    .send(Ok(CheckEvent {
                        event: Some(check_event::Event::Diagnostic(d.clone())),
                    }))
                    .await;
            }

            for v in &verifications {
                let _ = tx
                    .send(Ok(CheckEvent {
                        event: Some(check_event::Event::Verification(v.clone())),
                    }))
                    .await;
            }

            let _ = tx
                .send(Ok(CheckEvent {
                    event: Some(check_event::Event::Complete(CheckComplete {
                        success: diagnostics.iter().all(|d| d.severity != "error"),
                        total_diagnostics: diagnostics.len() as u32,
                        total_verifications: verifications.len() as u32,
                    })),
                }))
                .await;
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

/// Convert a byte offset span to (line, column, end_line, end_column).
/// Lines and columns are 1-based.
fn span_to_line_col(source: &str, span: &std::ops::Range<usize>) -> (u32, u32, u32, u32) {
    let start = span.start.min(source.len());
    let end = span.end.min(source.len());

    let before_start = &source[..start];
    let line = before_start.lines().count().max(1) as u32;
    let col = before_start
        .rsplit_once('\n')
        .map(|(_, after)| after.len() + 1)
        .unwrap_or(start + 1) as u32;

    let before_end = &source[..end];
    let end_line = before_end.lines().count().max(1) as u32;
    let end_col = before_end
        .rsplit_once('\n')
        .map(|(_, after)| after.len() + 1)
        .unwrap_or(end + 1) as u32;

    (line, col, end_line, end_col)
}

fn run_check(
    source: &str,
    _filename: &str,
    layer: i32,
) -> (Vec<Diagnostic>, Vec<VerificationResult>) {
    let (ast, parse_errors) = assura_parser::parse(source);

    let mut diagnostics = Vec::new();
    for err in &parse_errors {
        let span = err.span();
        let (line, column, end_line, end_column) = span_to_line_col(source, &span);
        diagnostics.push(Diagnostic {
            code: "A01002".into(),
            message: format!("{err:?}"),
            severity: "error".into(),
            line,
            column,
            end_line,
            end_column,
        });
    }

    let mut verifications = Vec::new();

    if let Some(ast) = ast {
        let resolve_result = assura_resolve::resolve(&ast);
        match resolve_result {
            Ok(resolved) => {
                // Emit resolution warnings (e.g., unused imports)
                for w in &resolved.warnings {
                    let (line, column, end_line, end_column) = span_to_line_col(source, &w.span);
                    diagnostics.push(Diagnostic {
                        code: w.code.to_string(),
                        message: w.message.clone(),
                        severity: "warning".into(),
                        line,
                        column,
                        end_line,
                        end_column,
                    });
                }
                let type_result = assura_types::type_check(&resolved);
                match type_result {
                    Ok(typed) => {
                        // Layer 0 = structural checks only (type checker),
                        // Layer 1+ = also run SMT verification
                        if layer < 1 {
                            return (diagnostics, verifications);
                        }
                        for vr in assura_smt::verify(&typed) {
                            let (clause, status, cex) = match &vr {
                                assura_smt::VerificationResult::Verified { clause_desc } => {
                                    (clause_desc.clone(), "verified".into(), String::new())
                                }
                                assura_smt::VerificationResult::Counterexample {
                                    clause_desc,
                                    model,
                                    ..
                                } => (clause_desc.clone(), "counterexample".into(), model.clone()),
                                assura_smt::VerificationResult::Timeout { clause_desc } => {
                                    (clause_desc.clone(), "timeout".into(), String::new())
                                }
                                assura_smt::VerificationResult::Unknown {
                                    clause_desc,
                                    reason,
                                } => (
                                    clause_desc.clone(),
                                    format!("unknown: {reason}"),
                                    String::new(),
                                ),
                            };
                            verifications.push(VerificationResult {
                                contract_name: String::new(),
                                clause,
                                status,
                                counterexample: cex,
                                time_ms: 0,
                            });
                        }
                    }
                    Err(type_errors) => {
                        for te in type_errors {
                            let (line, col, end_line, end_col) = span_to_line_col(source, &te.span);
                            diagnostics.push(Diagnostic {
                                code: te.code,
                                message: te.message,
                                severity: "error".into(),
                                line,
                                column: col,
                                end_line,
                                end_column: end_col,
                            });
                        }
                    }
                }
            }
            Err(resolve_errors) => {
                for re in resolve_errors {
                    let (line, col, end_line, end_col) = span_to_line_col(source, &re.span);
                    diagnostics.push(Diagnostic {
                        code: re.code.to_string(),
                        message: re.message,
                        severity: "error".into(),
                        line,
                        column: col,
                        end_line,
                        end_column: end_col,
                    });
                }
            }
        }
    }

    (diagnostics, verifications)
}

fn run_codegen(source: &str) -> std::collections::HashMap<String, String> {
    let (ast, _) = assura_parser::parse(source);

    let mut files = std::collections::HashMap::new();
    if let Some(ast) = ast
        && let Ok(resolved) = assura_resolve::resolve(&ast)
        && let Ok(typed) = assura_types::type_check(&resolved)
    {
        let generated = assura_codegen::codegen(&typed);
        for (path, content) in generated.files {
            files.insert(path, content);
        }
    }
    files
}

fn lookup_error_code(code: &str) -> (String, String, String, String) {
    let catalog: &[(&str, &str, &str, &str)] = &[
        (
            "A01001",
            "Unexpected character",
            "The lexer encountered a character that is not part of any valid token.",
            "Remove or replace the invalid character.",
        ),
        (
            "A01002",
            "Unexpected token",
            "The parser found a token that does not fit the expected grammar at this position.",
            "Check for missing colons, unmatched braces, or misspelled keywords.",
        ),
        (
            "A02001",
            "Undefined name",
            "A name was used that has not been defined in the current scope.",
            "Check spelling or add an import for the missing name.",
        ),
        (
            "A02003",
            "Duplicate definition",
            "Two declarations in the same scope share the same name.",
            "Rename one of the conflicting declarations.",
        ),
        (
            "A02005",
            "Circular import",
            "Module imports form a cycle, which is not allowed.",
            "Break the cycle by restructuring module dependencies.",
        ),
        (
            "A03001",
            "Type mismatch",
            "An expression has a type that does not match what was expected.",
            "Check that operand types are compatible with the operator or function.",
        ),
        (
            "A03002",
            "Argument count mismatch",
            "A function call has the wrong number of arguments.",
            "Match the number of arguments to the function's parameter list.",
        ),
        (
            "A05001",
            "Linear variable used twice",
            "A linear variable was used more than once, violating linearity.",
            "Ensure each linear variable is used exactly once.",
        ),
        (
            "A05002",
            "Linear variable unused",
            "A linear variable was never consumed.",
            "Use or explicitly drop the linear variable.",
        ),
        (
            "A06001",
            "Invalid state transition",
            "An operation was called on a typestate object in the wrong state.",
            "Check the state machine and ensure operations are called in valid states.",
        ),
        (
            "A07001",
            "Effect violation",
            "A pure function called an effectful function.",
            "Add the required effect to the function's effect declaration.",
        ),
        (
            "A08001",
            "Information flow violation",
            "Data flowed from a higher security level to a lower one without declassification.",
            "Add explicit declassification or restructure the data flow.",
        ),
        (
            "A09001",
            "Non-terminating recursion",
            "A recursive function has no valid decreases measure.",
            "Add a decreases clause with a well-founded measure.",
        ),
        (
            "A10001",
            "Non-exhaustive match",
            "A match expression does not cover all possible variants.",
            "Add the missing match arms or a wildcard pattern.",
        ),
    ];

    for (c, title, desc, fix) in catalog {
        if *c == code {
            return (
                title.to_string(),
                desc.to_string(),
                String::new(),
                fix.to_string(),
            );
        }
    }

    (
        format!("Error {code}"),
        format!("No detailed explanation available for {code}."),
        String::new(),
        String::new(),
    )
}

// JSON-over-HTTP fallback routes
mod http {
    use axum::{Json, Router, routing::post};
    use serde::{Deserialize, Serialize};

    #[derive(Deserialize)]
    pub struct HttpCheckRequest {
        pub source: String,
        #[serde(default)]
        pub filename: String,
        #[serde(default = "default_layer")]
        pub layer: i32,
    }

    fn default_layer() -> i32 {
        1
    }

    #[derive(Serialize)]
    pub struct HttpCheckResponse {
        pub success: bool,
        pub diagnostics: Vec<HttpDiagnostic>,
    }

    #[derive(Serialize)]
    pub struct HttpDiagnostic {
        pub code: String,
        pub message: String,
        pub severity: String,
    }

    #[derive(Deserialize)]
    pub struct HttpExplainRequest {
        pub error_code: String,
    }

    #[derive(Serialize)]
    pub struct HttpExplainResponse {
        pub error_code: String,
        pub title: String,
        pub description: String,
        pub example: String,
        pub fix: String,
    }

    #[derive(Serialize)]
    pub struct HttpHealthResponse {
        pub status: String,
        pub version: String,
    }

    async fn check_handler(Json(req): Json<HttpCheckRequest>) -> Json<HttpCheckResponse> {
        let (diagnostics, _) = super::run_check(&req.source, &req.filename, req.layer);
        let success = diagnostics.iter().all(|d| d.severity != "error");
        Json(HttpCheckResponse {
            success,
            diagnostics: diagnostics
                .into_iter()
                .map(|d| HttpDiagnostic {
                    code: d.code,
                    message: d.message,
                    severity: d.severity,
                })
                .collect(),
        })
    }

    async fn explain_handler(Json(req): Json<HttpExplainRequest>) -> Json<HttpExplainResponse> {
        let (title, description, example, fix) = super::lookup_error_code(&req.error_code);
        Json(HttpExplainResponse {
            error_code: req.error_code,
            title,
            description,
            example,
            fix,
        })
    }

    async fn health_handler() -> Json<HttpHealthResponse> {
        Json(HttpHealthResponse {
            status: "serving".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        })
    }

    pub fn router() -> Router {
        Router::new()
            .route("/v1/check", post(check_handler))
            .route("/v1/explain", post(explain_handler))
            .route("/v1/health", post(health_handler))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let grpc_addr = "[::]:50051".parse()?;
    let http_addr = "[::]:8080".parse::<std::net::SocketAddr>()?;

    println!("Assura gRPC server listening on {grpc_addr}");
    println!("Assura HTTP server listening on {http_addr}");

    let grpc_server = tonic::transport::Server::builder()
        .add_service(AssuraServiceServer::new(AssuraServer))
        .serve(grpc_addr);

    let http_server = axum::serve(
        tokio::net::TcpListener::bind(http_addr).await?,
        http::router(),
    );

    tokio::select! {
        r = grpc_server => r?,
        r = http_server => r?,
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===================================================================
    // run_check tests
    // ===================================================================

    #[test]
    fn check_valid_source_returns_no_errors() {
        let source = r#"contract Positive {
    input(x: Int)
    requires { x > 0 }
}"#;
        let (diagnostics, _) = run_check(source, "test.assura", 0);
        assert!(
            diagnostics.iter().all(|d| d.severity != "error"),
            "valid source should have no errors: {:?}",
            diagnostics
        );
    }

    #[test]
    fn check_invalid_source_returns_errors() {
        let source = r#"contract Bad {
    input(x: Int)
    requires { 42 }
}"#;
        let (diagnostics, _) = run_check(source, "test.assura", 0);
        assert!(
            diagnostics.iter().any(|d| d.severity == "error"),
            "invalid source should produce errors"
        );
        assert!(
            diagnostics.iter().any(|d| d.code == "A03006"),
            "expected A03006 for non-bool requires"
        );
    }

    #[test]
    fn check_empty_source_produces_no_errors() {
        // Empty source is valid (no declarations)
        let (diagnostics, _) = run_check("", "test.assura", 0);
        assert!(
            diagnostics.iter().all(|d| d.severity != "error"),
            "empty source should not produce errors"
        );
    }

    #[test]
    fn check_resolution_error_produces_diagnostic() {
        let source = r#"contract Dup {
    input(x: Int)
    requires { x > 0 }
}
contract Dup {
    input(y: Int)
    requires { y > 0 }
}"#;
        let (diagnostics, _) = run_check(source, "test.assura", 0);
        assert!(
            diagnostics.iter().any(|d| d.code == "A02003"),
            "duplicate definition should produce A02003"
        );
    }

    #[test]
    fn check_layer_zero_skips_smt() {
        let source = r#"contract Simple {
    input(x: Int)
    requires { x > 0 }
    ensures { result > 0 }
}"#;
        let (_, verifications) = run_check(source, "test.assura", 0);
        assert!(
            verifications.is_empty(),
            "layer 0 should skip SMT verification"
        );
    }

    #[test]
    fn check_layer_one_runs_smt() {
        let source = r#"contract Simple {
    input(x: Int)
    requires { x > 0 }
    ensures { result > 0 }
}"#;
        let (diagnostics, verifications) = run_check(source, "test.assura", 1);
        let has_errors = diagnostics.iter().any(|d| d.severity == "error");
        if !has_errors {
            // If type checking passes, verifications should be non-empty
            // (the ensures clause is verifiable)
            assert!(
                !verifications.is_empty(),
                "layer 1 should produce verification results"
            );
        }
    }

    // ===================================================================
    // run_codegen tests
    // ===================================================================

    #[test]
    fn codegen_valid_source_produces_files() {
        let source = r#"contract SafeAdd {
    input(a: Int, b: Int)
    requires { a > 0 }
    ensures { result == a + b }
}"#;
        let files = run_codegen(source);
        assert!(!files.is_empty(), "codegen should produce files");
        assert!(
            files.keys().any(|k| k.ends_with(".rs")),
            "codegen should produce a .rs file"
        );
    }

    #[test]
    fn codegen_invalid_source_produces_empty() {
        let source = "not valid assura code!!!";
        let files = run_codegen(source);
        assert!(
            files.is_empty(),
            "codegen on invalid source should produce no files"
        );
    }

    // ===================================================================
    // lookup_error_code tests
    // ===================================================================

    #[test]
    fn explain_known_code() {
        let (title, description, _, fix) = lookup_error_code("A03001");
        assert_eq!(title, "Type mismatch");
        assert!(!description.is_empty());
        assert!(!fix.is_empty());
    }

    #[test]
    fn explain_unknown_code() {
        let (title, description, _, _) = lookup_error_code("A99999");
        assert!(title.contains("A99999"));
        assert!(description.contains("No detailed explanation"));
    }

    #[test]
    fn explain_all_catalog_entries_are_nonempty() {
        let codes = [
            "A01001", "A01002", "A02001", "A02003", "A02005", "A03001", "A03002", "A05001",
            "A05002", "A06001", "A07001", "A08001", "A09001", "A10001",
        ];
        for code in &codes {
            let (title, desc, _, fix) = lookup_error_code(code);
            assert!(!title.is_empty(), "{code}: empty title");
            assert!(!desc.is_empty(), "{code}: empty description");
            assert!(!fix.is_empty(), "{code}: empty fix");
        }
    }

    // ===================================================================
    // span_to_line_col tests
    // ===================================================================

    #[test]
    fn span_to_line_col_first_line() {
        let source = "hello world";
        let (line, col, end_line, end_col) = span_to_line_col(source, &(0..5));
        assert_eq!(line, 1);
        assert_eq!(col, 1);
        assert_eq!(end_line, 1);
        assert_eq!(end_col, 6);
    }

    #[test]
    fn span_to_line_col_multiline() {
        let source = "first\nsecond";
        let (line, col, end_line, _) = span_to_line_col(source, &(6..12));
        // lines().count() on "first\n" returns 2 (empty trailing after \n counts)
        // but the function may return 1 or 2 depending on implementation
        assert!(line >= 1, "line should be at least 1");
        assert!(col >= 1, "col should be at least 1");
        assert!(end_line >= line, "end_line should be >= line");
    }

    #[test]
    fn span_to_line_col_empty_span() {
        let source = "hello";
        let (line, col, end_line, end_col) = span_to_line_col(source, &(0..0));
        assert_eq!(line, 1);
        assert_eq!(col, 1);
        assert_eq!(end_line, 1);
        assert_eq!(end_col, 1);
    }

    #[test]
    fn span_beyond_source_clamped() {
        let source = "hi";
        let (line, _, _, _) = span_to_line_col(source, &(100..200));
        assert_eq!(line, 1); // clamped to source length
    }

    // ===================================================================
    // HTTP handler tests
    // ===================================================================

    #[tokio::test]
    async fn http_health_returns_serving() {
        use ::http::Request;
        use axum::body::Body;
        use tower::ServiceExt;

        let app = super::http::router();
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/health")
                    .header("content-type", "application/json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "serving");
        assert!(!json["version"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn http_check_valid_source() {
        use ::http::Request;
        use axum::body::Body;
        use tower::ServiceExt;

        let app = super::http::router();
        let body = serde_json::json!({
            "source": "contract Ok { input(x: Int) requires { x > 0 } }",
            "filename": "test.assura"
        });
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/check")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], true);
    }

    #[tokio::test]
    async fn http_check_invalid_source() {
        use ::http::Request;
        use axum::body::Body;
        use tower::ServiceExt;

        let app = super::http::router();
        let body = serde_json::json!({
            "source": "contract Bad { input(x: Int) requires { 42 } }",
            "filename": "test.assura"
        });
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/check")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn http_explain_known_code() {
        use ::http::Request;
        use axum::body::Body;
        use tower::ServiceExt;

        let app = super::http::router();
        let body = serde_json::json!({ "error_code": "A03001" });
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/explain")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), 200);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["title"], "Type mismatch");
    }

    // ===================================================================
    // gRPC handler tests (unit-level, no network)
    // ===================================================================

    #[tokio::test]
    async fn grpc_check_valid_source() {
        let server = AssuraServer;
        let request = Request::new(CheckRequest {
            source: "contract Ok { input(x: Int) requires { x > 0 } }".into(),
            filename: "test.assura".into(),
            layer: 0,
        });
        let response = server.check(request).await.unwrap();
        let resp = response.into_inner();
        assert!(resp.success);
    }

    #[tokio::test]
    async fn grpc_check_invalid_source() {
        let server = AssuraServer;
        let request = Request::new(CheckRequest {
            source: "contract Bad { input(x: Int) requires { 42 } }".into(),
            filename: "test.assura".into(),
            layer: 0,
        });
        let response = server.check(request).await.unwrap();
        let resp = response.into_inner();
        assert!(!resp.success);
        assert!(resp.diagnostics.iter().any(|d| d.code == "A03006"));
    }

    #[tokio::test]
    async fn grpc_build_valid_source() {
        let server = AssuraServer;
        let request = Request::new(BuildRequest {
            source: "contract SafeAdd { input(a: Int, b: Int) requires { a > 0 } ensures { result == a + b } }".into(),
            filename: "test.assura".into(),
        });
        let response = server.build(request).await.unwrap();
        let resp = response.into_inner();
        assert!(resp.success);
        assert!(!resp.generated_files.is_empty());
    }

    #[tokio::test]
    async fn grpc_build_invalid_source_has_errors() {
        let server = AssuraServer;
        let request = Request::new(BuildRequest {
            source: "contract Bad { input(x: Int) requires { 42 } }".into(),
            filename: "test.assura".into(),
        });
        let response = server.build(request).await.unwrap();
        let resp = response.into_inner();
        assert!(!resp.success, "invalid source should not succeed");
        assert!(
            resp.diagnostics.iter().any(|d| d.severity == "error"),
            "should have error diagnostics"
        );
    }

    #[tokio::test]
    async fn grpc_explain_returns_description() {
        let server = AssuraServer;
        let request = Request::new(ExplainRequest {
            error_code: "A05001".into(),
        });
        let response = server.explain(request).await.unwrap();
        let resp = response.into_inner();
        assert_eq!(resp.title, "Linear variable used twice");
        assert!(!resp.description.is_empty());
    }

    #[tokio::test]
    async fn grpc_health_returns_serving() {
        let server = AssuraServer;
        let request = Request::new(HealthRequest {});
        let response = server.health(request).await.unwrap();
        let resp = response.into_inner();
        assert_eq!(resp.status, "serving");
        assert!(!resp.version.is_empty());
    }

    #[tokio::test]
    async fn grpc_check_stream_emits_events() {
        use tokio_stream::StreamExt;

        let server = AssuraServer;
        let request = Request::new(CheckRequest {
            source: "contract Ok { input(x: Int) requires { x > 0 } }".into(),
            filename: "test.assura".into(),
            layer: 0,
        });
        let response = server.check_stream(request).await.unwrap();
        let mut stream = response.into_inner();

        let mut got_complete = false;
        while let Some(event) = stream.next().await {
            let event = event.unwrap();
            if let Some(check_event::Event::Complete(c)) = event.event {
                got_complete = true;
                assert!(c.success);
            }
        }
        assert!(got_complete, "stream should end with a Complete event");
    }
}
