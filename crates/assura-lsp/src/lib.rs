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
use assura_resolve::{ResolvedFile, SymbolKind, SymbolTable};
use assura_types::TypeEnv;

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

    /// Parse, resolve, HIR lower, and type-check a document, publishing diagnostics.
    async fn analyze_document(&self, uri: &Url, text: &str) {
        let rope = Rope::from_str(text);
        let filename = uri.path();

        // Run the canonical pipeline (parse -> resolve -> HIR -> type check)
        let output =
            assura_pipeline::compile(text, filename, &assura_config::CompilerConfig::default());

        // Convert pipeline diagnostics to LSP diagnostics
        let diagnostics: Vec<Diagnostic> = output
            .diagnostics
            .iter()
            .map(|d| {
                let range = byte_span_to_range(&rope, &d.primary);
                let severity = Some(match d.severity {
                    assura_diagnostics::Severity::Error => DiagnosticSeverity::ERROR,
                    assura_diagnostics::Severity::Warning => DiagnosticSeverity::WARNING,
                    assura_diagnostics::Severity::Info => DiagnosticSeverity::INFORMATION,
                });
                let related_information = if d.secondary.is_empty() {
                    None
                } else {
                    Some(
                        d.secondary
                            .iter()
                            .map(|s| DiagnosticRelatedInformation {
                                location: Location::new(
                                    uri.clone(),
                                    byte_span_to_range(&rope, &s.span),
                                ),
                                message: s.message.clone(),
                            })
                            .collect(),
                    )
                };
                Diagnostic {
                    range,
                    severity,
                    code: Some(NumberOrString::String(d.code.to_string())),
                    source: Some("assura".to_string()),
                    message: d.message.clone(),
                    related_information,
                    ..Default::default()
                }
            })
            .collect();

        let state = DocumentState {
            rope,
            ast: output.file,
            resolved: output.resolved,
            type_env: output.typed.map(|t| t.type_env),
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
                document_formatting_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(false),
                    work_done_progress_options: WorkDoneProgressOptions {
                        work_done_progress: None,
                    },
                })),
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
                SymbolKind::BindFn => "bind function",
                SymbolKind::BuiltinType => "built-in type",
                SymbolKind::Operation => "operation",
                SymbolKind::Query => "query",
                SymbolKind::Parameter => "parameter",
                SymbolKind::TypeParam => "type parameter",
                SymbolKind::Field => "field",
                SymbolKind::EnumVariant => "enum variant",
                SymbolKind::Prophecy => "ghost prophecy",
                SymbolKind::CodecRegistry => "codec registry",
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

        // Effect name completions
        for effect in EFFECT_NAMES {
            items.push(CompletionItem {
                label: effect.to_string(),
                kind: Some(CompletionItemKind::VALUE),
                detail: Some("effect".to_string()),
                ..Default::default()
            });
        }

        // Snippet completions for common constructs
        for (label, snippet, detail) in SNIPPETS {
            items.push(CompletionItem {
                label: label.to_string(),
                kind: Some(CompletionItemKind::SNIPPET),
                detail: Some(detail.to_string()),
                insert_text: Some(snippet.to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                sort_text: Some(format!("0_{label}")),
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
                    SymbolKind::FnDef | SymbolKind::ExternFn | SymbolKind::BindFn => {
                        CompletionItemKind::FUNCTION
                    }
                    SymbolKind::Operation | SymbolKind::Query => CompletionItemKind::METHOD,
                    SymbolKind::Parameter => CompletionItemKind::VARIABLE,
                    SymbolKind::TypeParam => CompletionItemKind::TYPE_PARAMETER,
                    SymbolKind::Field => CompletionItemKind::FIELD,
                    SymbolKind::EnumVariant => CompletionItemKind::ENUM_MEMBER,
                    SymbolKind::Prophecy => CompletionItemKind::VARIABLE,
                    SymbolKind::CodecRegistry => CompletionItemKind::MODULE,
                    SymbolKind::BuiltinType => CompletionItemKind::CLASS,
                };
                let detail = match sym.kind {
                    SymbolKind::ContractDef => "contract",
                    SymbolKind::ServiceDef => "service",
                    SymbolKind::TypeDef => "type",
                    SymbolKind::EnumDef => "enum",
                    SymbolKind::FnDef => "function",
                    SymbolKind::ExternFn => "extern function",
                    SymbolKind::BindFn => "bind function",
                    SymbolKind::Operation => "operation",
                    SymbolKind::Query => "query",
                    SymbolKind::Parameter => "parameter",
                    SymbolKind::TypeParam => "type parameter",
                    SymbolKind::Field => "field",
                    SymbolKind::EnumVariant => "enum variant",
                    SymbolKind::Prophecy => "ghost prophecy",
                    SymbolKind::CodecRegistry => "codec registry",
                    SymbolKind::BuiltinType => "builtin type",
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

        let symbols = collect_document_symbols(ast, &state.rope, uri);

        Ok(Some(DocumentSymbolResponse::Flat(symbols)))
    }

    // --- Formatting ---

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = &params.text_document.uri;

        let state = match self.documents.get(uri) {
            Some(s) => s.clone(),
            None => return Ok(None),
        };

        let source = state.rope.to_string();

        // Parse the document; if parsing fails, return no edits to avoid breaking the document
        let (ast, errors) = assura_parser::parse(&source);
        if !errors.is_empty() {
            return Ok(Some(Vec::new()));
        }

        let ast = match ast {
            Some(a) => a,
            None => return Ok(Some(Vec::new())),
        };

        let formatted = assura_fmt::format_source_file(&ast);

        // If already formatted, return no edits
        if formatted == source {
            return Ok(Some(Vec::new()));
        }

        // Return a single edit replacing the full document
        let last_line = state.rope.len_lines().saturating_sub(1) as u32;
        let last_col = state.rope.line(last_line as usize).len_bytes() as u32;
        let full_range = Range {
            start: Position::new(0, 0),
            end: Position::new(last_line, last_col),
        };

        Ok(Some(vec![TextEdit {
            range: full_range,
            new_text: formatted,
        }]))
    }

    // --- Find References ---

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = &params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;

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

        // Find the word at the cursor position
        let word = match word_at_offset(&source, offset) {
            Some(w) => w,
            None => return Ok(None),
        };

        // Check the word exists as a symbol
        let scope_id = if resolved.symbols.scopes.len() > 1 {
            1
        } else {
            0
        };
        if resolved.symbols.lookup(&word, scope_id).is_none() {
            return Ok(None);
        }

        // Find all occurrences of this identifier in the source
        let locations = find_identifier_occurrences(&source, &word, &state.rope, uri);

        if locations.is_empty() {
            Ok(None)
        } else {
            Ok(Some(locations))
        }
    }

    // --- Rename ---

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = &params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let new_name = &params.new_name;

        // Validate the new name is a valid identifier
        if new_name.is_empty() || !is_valid_identifier(new_name) {
            return Err(tower_lsp::jsonrpc::Error::invalid_params(
                "new name must be a valid identifier",
            ));
        }

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

        // Find the word at the cursor position
        let word = match word_at_offset(&source, offset) {
            Some(w) => w,
            None => return Ok(None),
        };

        // Check the word exists as a symbol
        let scope_id = if resolved.symbols.scopes.len() > 1 {
            1
        } else {
            0
        };
        if resolved.symbols.lookup(&word, scope_id).is_none() {
            return Ok(None);
        }

        // Find all occurrences and create text edits
        let occurrences = find_identifier_occurrences(&source, &word, &state.rope, uri);

        let edits: Vec<TextEdit> = occurrences
            .into_iter()
            .map(|loc| TextEdit {
                range: loc.range,
                new_text: new_name.clone(),
            })
            .collect();

        if edits.is_empty() {
            return Ok(None);
        }

        let mut changes = std::collections::HashMap::new();
        changes.insert(uri.clone(), edits);

        Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }))
    }
}

// ---------------------------------------------------------------------------
// Reference and rename helpers
// ---------------------------------------------------------------------------

/// Find all word-boundary occurrences of an identifier in the source text.
fn find_identifier_occurrences(source: &str, name: &str, rope: &Rope, uri: &Url) -> Vec<Location> {
    let mut locations = Vec::new();
    let name_len = name.len();
    let bytes = source.as_bytes();

    let mut start = 0;
    while let Some(pos) = source[start..].find(name) {
        let abs_pos = start + pos;
        let end_pos = abs_pos + name_len;

        // Check word boundaries: must not be preceded or followed by ident chars
        let preceded_by_ident = abs_pos > 0 && is_ident_char(bytes[abs_pos - 1]);
        let followed_by_ident = end_pos < bytes.len() && is_ident_char(bytes[end_pos]);

        if !preceded_by_ident && !followed_by_ident {
            let range = byte_span_to_range(rope, &(abs_pos..end_pos));
            locations.push(Location::new(uri.clone(), range));
        }

        start = abs_pos + 1;
    }

    locations
}

/// Check if a string is a valid Assura identifier.
fn is_valid_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

// ---------------------------------------------------------------------------
// Document symbol collection
// ---------------------------------------------------------------------------

/// Collect top-level declarations as flat `SymbolInformation` entries.
#[allow(deprecated)] // SymbolInformation::deprecated is deprecated but required by the type
fn collect_document_symbols(
    ast: &SourceFile,
    rope: &Rope,
    doc_uri: &Url,
) -> Vec<SymbolInformation> {
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
                    location: Location::new(doc_uri.clone(), range),
                    container_name: None,
                });
            }
            Decl::Service(s) => {
                result.push(SymbolInformation {
                    name: s.name.clone(),
                    kind: SymbolKind2::MODULE,
                    tags: None,
                    deprecated: None,
                    location: Location::new(doc_uri.clone(), range),
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
                            location: Location::new(doc_uri.clone(), range),
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
                    location: Location::new(doc_uri.clone(), range),
                    container_name: None,
                });
            }
            Decl::EnumDef(e) => {
                result.push(SymbolInformation {
                    name: e.name.clone(),
                    kind: SymbolKind2::ENUM,
                    tags: None,
                    deprecated: None,
                    location: Location::new(doc_uri.clone(), range),
                    container_name: None,
                });
            }
            Decl::FnDef(f) => {
                result.push(SymbolInformation {
                    name: f.name.clone(),
                    kind: SymbolKind2::FUNCTION,
                    tags: None,
                    deprecated: None,
                    location: Location::new(doc_uri.clone(), range),
                    container_name: None,
                });
            }
            Decl::Extern(ex) => {
                result.push(SymbolInformation {
                    name: ex.name.clone(),
                    kind: SymbolKind2::FUNCTION,
                    tags: None,
                    deprecated: None,
                    location: Location::new(doc_uri.clone(), range),
                    container_name: None,
                });
            }
            Decl::Bind(b) => {
                result.push(SymbolInformation {
                    name: b.name.clone(),
                    kind: SymbolKind2::FUNCTION,
                    tags: None,
                    deprecated: None,
                    location: Location::new(doc_uri.clone(), range),
                    container_name: None,
                });
            }
            Decl::Prophecy(p) => {
                result.push(SymbolInformation {
                    name: p.name.clone(),
                    kind: SymbolKind2::VARIABLE,
                    tags: None,
                    deprecated: None,
                    location: Location::new(doc_uri.clone(), range),
                    container_name: None,
                });
            }
            Decl::CodecRegistry(cr) => {
                result.push(SymbolInformation {
                    name: cr.name.clone(),
                    kind: SymbolKind2::MODULE,
                    tags: None,
                    deprecated: None,
                    location: Location::new(doc_uri.clone(), range),
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
                        location: Location::new(doc_uri.clone(), range),
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
// Contract overlay support (custom LSP request)
// ---------------------------------------------------------------------------

/// Response for the `assura/contractOverlay` custom request.
///
/// Returns inline contract annotations found in a Rust source file,
/// suitable for rendering as virtual text decorations in the editor.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ContractOverlayResponse {
    pub items: Vec<ContractOverlayItem>,
}

/// A single contract overlay item for a function/struct/impl in a Rust file.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ContractOverlayItem {
    /// Display name (function/struct name).
    pub name: String,
    /// Line number (0-based) where the item starts.
    pub line: u32,
    /// The kind of annotated item ("function", "struct", "impl").
    pub kind: String,
    /// Contract clauses to display as overlay text.
    pub clauses: Vec<OverlayClause>,
}

/// A single clause for overlay display.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OverlayClause {
    /// Clause kind: "requires", "ensures", "invariant", "effects", "decreases".
    pub kind: String,
    /// The predicate body text.
    pub body: String,
}

/// Extract contract overlay data from a Rust source file.
pub fn get_contract_overlays(source: &str) -> ContractOverlayResponse {
    let items = match assura_rust_analyzer::parse_rust_source(source) {
        Ok(items) => items,
        Err(_) => return ContractOverlayResponse { items: Vec::new() },
    };

    let overlay_items = items
        .into_iter()
        .map(|item| {
            let (name, kind) = match &item.kind {
                assura_rust_analyzer::AnnotatedItemKind::Function { name, .. } => {
                    (name.clone(), "function".to_string())
                }
                assura_rust_analyzer::AnnotatedItemKind::Struct { name, .. } => {
                    (name.clone(), "struct".to_string())
                }
                assura_rust_analyzer::AnnotatedItemKind::ImplBlock {
                    self_type,
                    trait_name,
                } => {
                    let name = match trait_name {
                        Some(t) => format!("{t} for {self_type}"),
                        None => self_type.clone(),
                    };
                    (name, "impl".to_string())
                }
            };

            let mut clauses = Vec::new();
            for c in &item.contract.requires {
                clauses.push(OverlayClause {
                    kind: "requires".to_string(),
                    body: c.body.clone(),
                });
            }
            for c in &item.contract.ensures {
                clauses.push(OverlayClause {
                    kind: "ensures".to_string(),
                    body: c.body.clone(),
                });
            }
            for c in &item.contract.invariants {
                clauses.push(OverlayClause {
                    kind: "invariant".to_string(),
                    body: c.body.clone(),
                });
            }
            for c in &item.contract.effects {
                clauses.push(OverlayClause {
                    kind: "effects".to_string(),
                    body: c.body.clone(),
                });
            }
            for c in &item.contract.decreases {
                clauses.push(OverlayClause {
                    kind: "decreases".to_string(),
                    body: c.body.clone(),
                });
            }

            ContractOverlayItem {
                name,
                line: item.line.saturating_sub(1) as u32, // convert to 0-based
                kind,
                clauses,
            }
        })
        .collect();

    ContractOverlayResponse {
        items: overlay_items,
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
    "bind",
    "prophecy",
    "match",
    "let",
    "abstract",
    "decreases",
    "variant",
    "axiom",
    "opaque",
    "linear",
    "unique",
    "trusted",
];

/// Known effect names from the Assura specification.
const EFFECT_NAMES: &[&str] = &[
    "pure",
    "io",
    "database",
    "logging",
    "mem",
    "net",
    "fs",
    "rng",
    "time",
    "alloc",
    "diverge",
    "random",
    "console.read",
    "console.write",
    "filesystem.read",
    "filesystem.write",
    "network.connect",
    "network.listen",
    "database.read",
    "database.write",
    "log.info",
    "log.warn",
    "log.error",
];

/// Snippet templates for common Assura constructs.
const SNIPPETS: &[(&str, &str, &str)] = &[
    (
        "contract",
        "contract ${1:Name} {\n    input(${2:x}: ${3:Int})\n    output(${4:result}: ${5:Int})\n    requires { ${6:true} }\n    ensures { ${7:true} }\n}",
        "Contract with input, output, requires, and ensures",
    ),
    (
        "service",
        "service ${1:Name} {\n    states: ${2:Init} -> ${3:Ready}\n\n    operation ${4:Do} {\n        requires: ${5:true}\n    }\n}",
        "Service with states and operations",
    ),
    (
        "fn",
        "fn ${1:name}(${2:x}: ${3:Int}) -> ${4:Int}\n    requires { ${5:true} }\n    ensures { ${6:true} }",
        "Function with pre/postconditions",
    ),
    (
        "extern fn",
        "extern fn ${1:name}(${2:x}: ${3:Int}) -> ${4:Int}\n    effects { ${5:io} }",
        "Extern function with effects",
    ),
    ("module", "module ${1:name}", "Module declaration"),
    (
        "import",
        "import ${1:module}.${2:Name}",
        "Import declaration",
    ),
    (
        "type",
        "type ${1:Name} {\n    ${2:field}: ${3:Int}\n}",
        "Type definition with fields",
    ),
    (
        "enum",
        "enum ${1:Name} {\n    ${2:Variant1}\n    ${3:Variant2}\n}",
        "Enum definition with variants",
    ),
];

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod lsp_tests;
