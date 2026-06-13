//! LSP server for the Assura contract language.
//!
//! Provides diagnostics (parse, resolve, type errors), go-to-definition,
//! hover, completion, and document symbols via the Language Server Protocol.

use std::sync::Arc;

use dashmap::DashMap;
use ropey::Rope;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use assura_parser::ast::{Decl, ServiceItem, SourceFile};
use assura_resolve::{ResolutionError, ResolvedFile, SymbolKind, SymbolTable};
use assura_types::{TypeEnv, TypeError};

// ---------------------------------------------------------------------------
// Per-document state
// ---------------------------------------------------------------------------

/// Cached analysis results for a single open document.
#[derive(Clone)]
struct DocumentState {
    /// Full source text as a rope (efficient for incremental edits).
    rope: Rope,
    /// Parsed AST (None if parsing failed entirely).
    ast: Option<SourceFile>,
    /// Resolved file (None if resolution was not attempted or failed).
    resolved: Option<ResolvedFile>,
    /// Type environment (None if type checking was not attempted or failed).
    type_env: Option<TypeEnv>,
}

// ---------------------------------------------------------------------------
// Language server
// ---------------------------------------------------------------------------

/// The Assura LSP server.
pub struct AssuraLanguageServer {
    client: Client,
    documents: Arc<DashMap<Url, DocumentState>>,
}

impl AssuraLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(DashMap::new()),
        }
    }

    /// Parse, resolve, and type-check a document, publishing diagnostics.
    async fn analyze_document(&self, uri: &Url, text: &str) {
        let rope = Rope::from_str(text);
        let mut diagnostics = Vec::new();

        // --- Parse ---
        let (ast, parse_errors) = assura_parser::parse(text);

        for err in &parse_errors {
            let range = byte_span_to_range(&rope, &err.span());
            let message = format!("{err}");
            diagnostics.push(Diagnostic {
                range,
                severity: Some(DiagnosticSeverity::ERROR),
                code: Some(NumberOrString::String("A01001".to_string())),
                source: Some("assura".to_string()),
                message,
                ..Default::default()
            });
        }

        let mut resolved: Option<ResolvedFile> = None;
        let mut type_env: Option<TypeEnv> = None;

        if let Some(ref source_file) = ast {
            // --- Resolve ---
            match assura_resolve::resolve(source_file) {
                Ok(rf) => {
                    // --- Type check ---
                    match assura_types::type_check(&rf) {
                        Ok(typed) => {
                            type_env = Some(typed.type_env);
                        }
                        Err(type_errors) => {
                            for te in &type_errors {
                                diagnostics.push(type_error_to_diagnostic(&rope, te));
                            }
                        }
                    }
                    resolved = Some(rf);
                }
                Err(res_errors) => {
                    for re in &res_errors {
                        diagnostics.push(resolution_error_to_diagnostic(&rope, re));
                    }
                }
            }
        }

        let state = DocumentState {
            rope,
            ast,
            resolved,
            type_env,
        };
        self.documents.insert(uri.clone(), state);

        self.client
            .publish_diagnostics(uri.clone(), diagnostics, None)
            .await;
    }

    /// Find the symbol at a given byte offset in the resolved symbol table.
    fn find_symbol_at<'a>(
        &self,
        symbols: &'a SymbolTable,
        source: &str,
        offset: usize,
    ) -> Option<(String, &'a assura_resolve::Symbol)> {
        // Get the word at offset
        let word = word_at_offset(source, offset)?;
        // Look up in symbol table from module scope (scope 1, child of root)
        let scope_id = if symbols.scopes.len() > 1 { 1 } else { 0 };
        let sym = symbols.lookup(&word, scope_id)?;
        Some((word, sym))
    }
}

// ---------------------------------------------------------------------------
// LanguageServer trait implementation
// ---------------------------------------------------------------------------

#[tower_lsp::async_trait]
impl LanguageServer for AssuraLanguageServer {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                definition_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".to_string(), ":".to_string()]),
                    ..Default::default()
                }),
                document_symbol_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "assura-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Assura LSP server initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    // --- Text document sync ---

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        self.analyze_document(&uri, &text).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        // We use FULL sync, so the first content change has the full text.
        if let Some(change) = params.content_changes.into_iter().next() {
            self.analyze_document(&uri, &change.text).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        // Re-read text from the stored rope (save doesn't always include text).
        if let Some(state) = self.documents.get(&uri) {
            let text = state.rope.to_string();
            drop(state);
            self.analyze_document(&uri, &text).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.remove(&uri);
        // Clear diagnostics for closed files.
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }

    // --- Go to Definition ---

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        let state = match self.documents.get(uri) {
            Some(s) => s.clone(),
            None => return Ok(None),
        };

        let resolved = match &state.resolved {
            Some(r) => r,
            None => return Ok(None),
        };

        let source = state.rope.to_string();
        let offset = position_to_offset(&state.rope, pos);

        if let Some((_word, sym)) = self.find_symbol_at(&resolved.symbols, &source, offset) {
            // Don't jump to built-in types (sentinel span 0..0)
            if sym.span.start == 0 && sym.span.end == 0 {
                return Ok(None);
            }
            let range = byte_span_to_range(&state.rope, &sym.span);
            let loc = Location::new(uri.clone(), range);
            return Ok(Some(GotoDefinitionResponse::Scalar(loc)));
        }

        Ok(None)
    }

    // --- Hover ---

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        let state = match self.documents.get(uri) {
            Some(s) => s.clone(),
            None => return Ok(None),
        };

        let resolved = match &state.resolved {
            Some(r) => r,
            None => return Ok(None),
        };

        let source = state.rope.to_string();
        let offset = position_to_offset(&state.rope, pos);

        if let Some((word, sym)) = self.find_symbol_at(&resolved.symbols, &source, offset) {
            let type_info = state
                .type_env
                .as_ref()
                .and_then(|env| env.lookup(&word))
                .map(|ty| format!("{ty:?}"))
                .unwrap_or_else(|| "unknown".to_string());

            let kind_label = match sym.kind {
                SymbolKind::ContractDef => "contract",
                SymbolKind::ServiceDef => "service",
                SymbolKind::TypeDef => "type",
                SymbolKind::EnumDef => "enum",
                SymbolKind::FnDef => "function",
                SymbolKind::ExternFn => "extern function",
                SymbolKind::BuiltinType => "built-in type",
                SymbolKind::Operation => "operation",
                SymbolKind::Query => "query",
                SymbolKind::Parameter => "parameter",
                SymbolKind::TypeParam => "type parameter",
                SymbolKind::Field => "field",
                SymbolKind::EnumVariant => "enum variant",
            };

            let hover_text = format!("**{kind_label}** `{word}`\n\nType: `{type_info}`");

            return Ok(Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: hover_text,
                }),
                range: None,
            }));
        }

        Ok(None)
    }

    // --- Completion ---

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;

        let state = match self.documents.get(uri) {
            Some(s) => s.clone(),
            None => return Ok(None),
        };

        let mut items = Vec::new();

        // Built-in type completions
        for name in BUILTIN_TYPES {
            items.push(CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::CLASS),
                detail: Some("built-in type".to_string()),
                ..Default::default()
            });
        }

        // Keyword completions
        for kw in KEYWORDS {
            items.push(CompletionItem {
                label: kw.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some("keyword".to_string()),
                ..Default::default()
            });
        }

        // Symbols from the resolved file
        if let Some(ref resolved) = state.resolved {
            for sym in &resolved.symbols.symbols {
                if sym.kind == SymbolKind::BuiltinType {
                    continue; // already added above
                }
                let kind = match sym.kind {
                    SymbolKind::ContractDef => CompletionItemKind::CLASS,
                    SymbolKind::ServiceDef => CompletionItemKind::MODULE,
                    SymbolKind::TypeDef => CompletionItemKind::CLASS,
                    SymbolKind::EnumDef => CompletionItemKind::ENUM,
                    SymbolKind::FnDef | SymbolKind::ExternFn => CompletionItemKind::FUNCTION,
                    SymbolKind::Operation | SymbolKind::Query => CompletionItemKind::METHOD,
                    SymbolKind::Parameter => CompletionItemKind::VARIABLE,
                    SymbolKind::TypeParam => CompletionItemKind::TYPE_PARAMETER,
                    SymbolKind::Field => CompletionItemKind::FIELD,
                    SymbolKind::EnumVariant => CompletionItemKind::ENUM_MEMBER,
                    SymbolKind::BuiltinType => unreachable!(),
                };
                let detail = match sym.kind {
                    SymbolKind::ContractDef => "contract",
                    SymbolKind::ServiceDef => "service",
                    SymbolKind::TypeDef => "type",
                    SymbolKind::EnumDef => "enum",
                    SymbolKind::FnDef => "function",
                    SymbolKind::ExternFn => "extern function",
                    SymbolKind::Operation => "operation",
                    SymbolKind::Query => "query",
                    SymbolKind::Parameter => "parameter",
                    SymbolKind::TypeParam => "type parameter",
                    SymbolKind::Field => "field",
                    SymbolKind::EnumVariant => "enum variant",
                    SymbolKind::BuiltinType => unreachable!(),
                };
                items.push(CompletionItem {
                    label: sym.name.clone(),
                    kind: Some(kind),
                    detail: Some(detail.to_string()),
                    ..Default::default()
                });
            }
        }

        Ok(Some(CompletionResponse::Array(items)))
    }

    // --- Document Symbols ---

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = &params.text_document.uri;

        let state = match self.documents.get(uri) {
            Some(s) => s.clone(),
            None => return Ok(None),
        };

        let ast = match &state.ast {
            Some(a) => a,
            None => return Ok(None),
        };

        let symbols = collect_document_symbols(ast, &state.rope);

        Ok(Some(DocumentSymbolResponse::Flat(symbols)))
    }
}

// ---------------------------------------------------------------------------
// Document symbol collection
// ---------------------------------------------------------------------------

/// Collect top-level declarations as flat `SymbolInformation` entries.
#[allow(deprecated)] // SymbolInformation::deprecated is deprecated but required by the type
fn collect_document_symbols(ast: &SourceFile, rope: &Rope) -> Vec<SymbolInformation> {
    let mut result = Vec::new();

    for decl in &ast.decls {
        let range = byte_span_to_range(rope, &decl.span);
        match &decl.node {
            Decl::Contract(c) => {
                result.push(SymbolInformation {
                    name: c.name.clone(),
                    kind: SymbolKind2::CLASS,
                    tags: None,
                    deprecated: None,
                    location: Location::new(Url::parse("file:///").unwrap(), range),
                    container_name: None,
                });
            }
            Decl::Service(s) => {
                result.push(SymbolInformation {
                    name: s.name.clone(),
                    kind: SymbolKind2::MODULE,
                    tags: None,
                    deprecated: None,
                    location: Location::new(Url::parse("file:///").unwrap(), range),
                    container_name: None,
                });
                // Add service items as children
                for item in &s.items {
                    let child_name = match item {
                        ServiceItem::TypeDef(t) => Some((t.name.clone(), SymbolKind2::CLASS)),
                        ServiceItem::EnumDef(e) => Some((e.name.clone(), SymbolKind2::ENUM)),
                        ServiceItem::Operation { name, .. } => {
                            Some((name.clone(), SymbolKind2::METHOD))
                        }
                        ServiceItem::Query { name, .. } => {
                            Some((name.clone(), SymbolKind2::METHOD))
                        }
                        _ => None,
                    };
                    if let Some((name, kind)) = child_name {
                        result.push(SymbolInformation {
                            name,
                            kind,
                            tags: None,
                            deprecated: None,
                            location: Location::new(Url::parse("file:///").unwrap(), range),
                            container_name: Some(s.name.clone()),
                        });
                    }
                }
            }
            Decl::TypeDef(t) => {
                result.push(SymbolInformation {
                    name: t.name.clone(),
                    kind: SymbolKind2::STRUCT,
                    tags: None,
                    deprecated: None,
                    location: Location::new(Url::parse("file:///").unwrap(), range),
                    container_name: None,
                });
            }
            Decl::EnumDef(e) => {
                result.push(SymbolInformation {
                    name: e.name.clone(),
                    kind: SymbolKind2::ENUM,
                    tags: None,
                    deprecated: None,
                    location: Location::new(Url::parse("file:///").unwrap(), range),
                    container_name: None,
                });
            }
            Decl::FnDef(f) => {
                result.push(SymbolInformation {
                    name: f.name.clone(),
                    kind: SymbolKind2::FUNCTION,
                    tags: None,
                    deprecated: None,
                    location: Location::new(Url::parse("file:///").unwrap(), range),
                    container_name: None,
                });
            }
            Decl::Extern(ex) => {
                result.push(SymbolInformation {
                    name: ex.name.clone(),
                    kind: SymbolKind2::FUNCTION,
                    tags: None,
                    deprecated: None,
                    location: Location::new(Url::parse("file:///").unwrap(), range),
                    container_name: None,
                });
            }
            Decl::Block { name, .. } => {
                if !name.is_empty() {
                    result.push(SymbolInformation {
                        name: name.clone(),
                        kind: SymbolKind2::NAMESPACE,
                        tags: None,
                        deprecated: None,
                        location: Location::new(Url::parse("file:///").unwrap(), range),
                        container_name: None,
                    });
                }
            }
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Helper: span/position conversions
// ---------------------------------------------------------------------------

use tower_lsp::lsp_types::SymbolKind as SymbolKind2;

/// Convert a byte span (Range<usize>) to an LSP Range (line/col).
fn byte_span_to_range(rope: &Rope, span: &std::ops::Range<usize>) -> Range {
    let start = byte_to_position(rope, span.start);
    let end = byte_to_position(rope, span.end);
    Range { start, end }
}

/// Convert a byte offset to an LSP Position (0-based line and character).
fn byte_to_position(rope: &Rope, byte_offset: usize) -> Position {
    let clamped = byte_offset.min(rope.len_bytes());
    let line = rope.byte_to_line(clamped);
    let line_start = rope.line_to_byte(line);
    let col = clamped - line_start;
    Position::new(line as u32, col as u32)
}

/// Convert an LSP Position to a byte offset.
fn position_to_offset(rope: &Rope, pos: Position) -> usize {
    let line = (pos.line as usize).min(rope.len_lines().saturating_sub(1));
    let line_start = rope.line_to_byte(line);
    let line_len = rope.line(line).len_bytes();
    let col = (pos.character as usize).min(line_len);
    line_start + col
}

/// Extract the word (identifier) at a given byte offset.
fn word_at_offset(source: &str, offset: usize) -> Option<String> {
    if offset > source.len() {
        return None;
    }
    let bytes = source.as_bytes();

    // Find start of word
    let mut start = offset;
    while start > 0 && is_ident_char(bytes[start - 1]) {
        start -= 1;
    }

    // Find end of word
    let mut end = offset;
    while end < bytes.len() && is_ident_char(bytes[end]) {
        end += 1;
    }

    if start == end {
        return None;
    }

    Some(source[start..end].to_string())
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

// ---------------------------------------------------------------------------
// Diagnostic conversion helpers
// ---------------------------------------------------------------------------

fn resolution_error_to_diagnostic(rope: &Rope, err: &ResolutionError) -> Diagnostic {
    let range = byte_span_to_range(rope, &err.span);
    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::ERROR),
        code: Some(NumberOrString::String(err.code.to_string())),
        source: Some("assura".to_string()),
        message: err.message.clone(),
        related_information: err.secondary.as_ref().map(|(sec_span, sec_msg)| {
            vec![DiagnosticRelatedInformation {
                location: Location::new(
                    Url::parse("file:///").unwrap(),
                    byte_span_to_range(rope, sec_span),
                ),
                message: sec_msg.clone(),
            }]
        }),
        ..Default::default()
    }
}

fn type_error_to_diagnostic(rope: &Rope, err: &TypeError) -> Diagnostic {
    let range = byte_span_to_range(rope, &err.span);
    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::ERROR),
        code: Some(NumberOrString::String(err.code.clone())),
        source: Some("assura".to_string()),
        message: err.message.clone(),
        related_information: err.secondary.as_ref().map(|(sec_span, sec_msg)| {
            vec![DiagnosticRelatedInformation {
                location: Location::new(
                    Url::parse("file:///").unwrap(),
                    byte_span_to_range(rope, sec_span),
                ),
                message: sec_msg.clone(),
            }]
        }),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Constants for completion
// ---------------------------------------------------------------------------

const BUILTIN_TYPES: &[&str] = &[
    "Int", "Nat", "Float", "Bool", "String", "Bytes", "Unit", "Never", "List", "Map", "Set",
    "Option", "Result", "U8", "U16", "U32", "U64", "I8", "I16", "I32", "I64", "F32", "F64",
    "Sequence",
];

const KEYWORDS: &[&str] = &[
    "contract",
    "service",
    "type",
    "enum",
    "fn",
    "extern",
    "requires",
    "ensures",
    "effects",
    "invariant",
    "modifies",
    "input",
    "output",
    "errors",
    "rule",
    "data-flow",
    "must-not",
    "import",
    "module",
    "project",
    "forall",
    "exists",
    "old",
    "result",
    "true",
    "false",
    "if",
    "then",
    "else",
    "and",
    "or",
    "not",
    "in",
    "ghost",
    "pure",
    "lemma",
    "pub",
    "mut",
    "operation",
    "query",
    "states",
    "as",
    "where",
];

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_byte_to_position_empty() {
        let rope = Rope::from_str("");
        let pos = byte_to_position(&rope, 0);
        assert_eq!(pos.line, 0);
        assert_eq!(pos.character, 0);
    }

    #[test]
    fn test_byte_to_position_multiline() {
        let rope = Rope::from_str("hello\nworld\n");
        // 'w' is at byte 6 (line 1, col 0)
        let pos = byte_to_position(&rope, 6);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 0);
        // 'o' is at byte 7 (line 1, col 1)
        let pos2 = byte_to_position(&rope, 7);
        assert_eq!(pos2.line, 1);
        assert_eq!(pos2.character, 1);
    }

    #[test]
    fn test_position_to_offset() {
        let rope = Rope::from_str("hello\nworld\n");
        let offset = position_to_offset(&rope, Position::new(1, 3));
        assert_eq!(offset, 9); // 6 (line start) + 3
    }

    #[test]
    fn test_word_at_offset() {
        let source = "contract Foo {";
        assert_eq!(word_at_offset(source, 0), Some("contract".to_string()));
        assert_eq!(word_at_offset(source, 9), Some("Foo".to_string()));
        assert_eq!(word_at_offset(source, 10), Some("Foo".to_string()));
        assert_eq!(word_at_offset(source, 13), None); // space after Foo
    }

    #[test]
    fn test_byte_span_to_range() {
        let rope = Rope::from_str("line one\nline two\n");
        let range = byte_span_to_range(&rope, &(9..17));
        assert_eq!(range.start.line, 1);
        assert_eq!(range.start.character, 0);
        assert_eq!(range.end.line, 1);
        assert_eq!(range.end.character, 8);
    }

    #[test]
    fn test_collect_document_symbols_basic() {
        let (ast, errors) = assura_parser::parse(
            r#"
contract Foo {
  requires { true }
}

type Bar {
  x: Int
}

enum Baz {
  A
  B
}

fn helper(n: Int) -> Int {
  ensures { result >= 0 }
}
"#,
        );
        assert!(errors.is_empty(), "parse errors: {errors:?}");
        let ast = ast.unwrap();
        let rope = Rope::from_str("");
        let symbols = collect_document_symbols(&ast, &rope);
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Foo"), "missing Foo");
        assert!(names.contains(&"Bar"), "missing Bar");
        assert!(names.contains(&"Baz"), "missing Baz");
        assert!(names.contains(&"helper"), "missing helper");
    }

    #[test]
    fn test_diagnostics_from_parse_errors() {
        // Intentionally malformed input: parse should produce errors
        let source = "contract { }";
        let (_, errors) = assura_parser::parse(source);
        // The parser should recover from missing contract name
        // We just verify we can convert errors to diagnostics without panic
        let rope = Rope::from_str(source);
        for err in &errors {
            let range = byte_span_to_range(&rope, &err.span());
            assert!(range.start.line <= range.end.line);
        }
    }

    #[test]
    fn test_resolution_error_diagnostic() {
        let err = ResolutionError {
            code: "A02001",
            message: "unknown type `Foo`".to_string(),
            span: 0..3,
            secondary: None,
        };
        let rope = Rope::from_str("Foo");
        let diag = resolution_error_to_diagnostic(&rope, &err);
        assert_eq!(diag.message, "unknown type `Foo`");
        assert_eq!(
            diag.code,
            Some(NumberOrString::String("A02001".to_string()))
        );
    }

    #[test]
    fn test_type_error_diagnostic() {
        let err = TypeError {
            code: "A03001".to_string(),
            message: "type mismatch".to_string(),
            span: 0..5,
            secondary: None,
        };
        let rope = Rope::from_str("hello");
        let diag = type_error_to_diagnostic(&rope, &err);
        assert_eq!(diag.message, "type mismatch");
        assert_eq!(
            diag.code,
            Some(NumberOrString::String("A03001".to_string()))
        );
    }
}
