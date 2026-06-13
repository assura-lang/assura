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
    _layer: i32,
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
                let type_result = assura_types::type_check(&resolved);
                match type_result {
                    Ok(typed) => {
                        // Run SMT verification on the type-checked file
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
