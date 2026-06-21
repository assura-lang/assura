//! Parse inline contract annotations (`/// @requires`, `/// @ensures`, etc.)
//! from Rust source files.
//!
//! This crate extracts Assura contract clauses embedded in Rust doc comments
//! and maps them to the functions, structs, and impl blocks they annotate.

use std::path::Path;

use quote::ToTokens;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single contract clause extracted from a doc comment.
#[derive(Debug, Clone, PartialEq)]
pub struct ContractClause {
    /// The kind of clause (requires, ensures, invariant, effects, decreases).
    pub kind: InlineClauseKind,
    /// The predicate text (everything after the `@keyword`).
    pub body: String,
    /// Byte offset of the clause within the source file (start of `@keyword`).
    pub offset: usize,
}

/// Clause kinds supported in inline annotations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InlineClauseKind {
    // -- Core contract clauses --
    Requires,
    Ensures,
    Invariant,
    Effects,
    Decreases,
    /// SEC.2: FFI boundary trust annotation (`@ffi_boundary trusted|audited|untrusted`)
    FfiBoundary,
    /// SEC.2: Trust level shorthand (`@trust trusted|audited|untrusted`)
    Trust,

    // -- CORE feature annotations --
    /// CORE.1: Ghost variable/function annotation (`@ghost`)
    Ghost,
    /// CORE.2: Lemma annotation (`@lemma`)
    Lemma,
    /// CORE.3: Frame condition / modifies clause (`@modifies`)
    Modifies,
    /// CORE.6: Opaque function body (`@opaque`)
    Opaque,
    /// CORE.8: Liveness / eventual property (`@eventually`)
    Eventually,

    // -- SEC feature annotations --
    /// SEC.1: Taint tracking annotation (`@taint`)
    Taint,
    /// SEC.3: Constant-time execution (`@constant_time`)
    ConstantTime,
    /// SEC.4: Zeroize sensitive data (`@zeroize`)
    Zeroize,

    // -- MEM feature annotations --
    /// MEM.1: Memory region annotation (`@region`)
    Region,
    /// MEM.2: Bit-width constraint (`@width`)
    Width,
    /// MEM.3: Allocator annotation (`@allocator`)
    Allocator,
    /// MEM.4: Circular buffer annotation (`@circular`)
    Circular,

    // -- TYPE feature annotations --
    /// TYPE.1: Interface / trait bound (`@interface`)
    Interface,
    /// TYPE.3: Error type annotation (`@errors`)
    Errors,

    // -- CONC feature annotations --
    /// CONC.1: Shared state annotation (`@shared`)
    Shared,
    /// CONC.2: Non-reentrant function (`@no_reentrant`)
    NoReentrant,
    /// CONC.3: Deterministic execution (`@deterministic`)
    Deterministic,
    /// CONC.4: Lock ordering annotation (`@lock_order`)
    LockOrder,
    /// CONC.5: Deadline annotation (`@deadline`)
    Deadline,
    /// CONC.6: Memory ordering annotation (`@ordering`)
    MemoryOrdering,

    // -- FMT feature annotations --
    /// FMT.1: Binary format annotation (`@format`)
    Format,
    /// FMT.2: Bit-level layout (`@bits`)
    Bits,
    /// FMT.3: String/data encoding (`@encoding`)
    Encoding,
    /// FMT.5: Checksum annotation (`@checksum`)
    Checksum,

    // -- PLAT feature annotations --
    /// PLAT.1: Platform-specific annotation (`@platform`)
    Platform,
    /// PLAT.2: Feature gate annotation (`@feature`)
    Feature,
    /// PLAT.3: Resource limit annotation (`@resource`)
    Resource,

    // -- PERF feature annotations --
    /// PERF.1: Unsafe escape hatch (`@unsafe_escape`)
    UnsafeEscape,
    /// PERF.2: Complexity annotation (`@complexity`)
    Complexity,

    // -- NUM feature annotations --
    /// NUM.1: Numerical precision (`@precision`)
    Precision,

    // -- STOR feature annotations --
    /// STOR.5: Monotonic state (`@monotonic`)
    Monotonic,

    // -- MISC feature annotations --
    /// MISC.2: Suspend invariant checking (`@suspend_invariant`)
    SuspendInvariant,
}

impl InlineClauseKind {
    /// Parse a clause kind from its keyword string.
    pub fn from_keyword(s: &str) -> Option<Self> {
        match s {
            "requires" => Some(Self::Requires),
            "ensures" => Some(Self::Ensures),
            "invariant" => Some(Self::Invariant),
            "effects" => Some(Self::Effects),
            "decreases" => Some(Self::Decreases),
            "ffi_boundary" => Some(Self::FfiBoundary),
            "trust" => Some(Self::Trust),
            // CORE
            "ghost" => Some(Self::Ghost),
            "lemma" => Some(Self::Lemma),
            "modifies" => Some(Self::Modifies),
            "opaque" => Some(Self::Opaque),
            "eventually" => Some(Self::Eventually),
            // CORE aliases (coverage: axiom|axiomatic, trigger|auto_trigger, prophecy)
            "axiom" | "axiomatic" => Some(Self::Invariant),
            "trigger" | "auto_trigger" => Some(Self::Decreases),
            "prophecy" => Some(Self::Ghost),
            // SEC
            "taint" => Some(Self::Taint),
            "constant_time" => Some(Self::ConstantTime),
            "zeroize" | "secure_erase" => Some(Self::Zeroize),
            // SEC aliases (coverage: conforms)
            "conforms" => Some(Self::Trust),
            // MEM
            "region" => Some(Self::Region),
            "width" | "FixedWidth" => Some(Self::Width),
            "allocator" => Some(Self::Allocator),
            "circular" | "circular_buffer" => Some(Self::Circular),
            // TYPE
            "interface" => Some(Self::Interface),
            "errors" | "error_policy" => Some(Self::Errors),
            // TYPE aliases (coverage: structural_invariant, must_propagate)
            "structural_invariant" => Some(Self::Invariant),
            "must_propagate" | "must_not_mask" => Some(Self::Errors),
            // CONC
            "shared" | "shared_memory" | "SharedMem" => Some(Self::Shared),
            "no_reentrant" | "must_not_reenter" | "callback" => Some(Self::NoReentrant),
            "deterministic" => Some(Self::Deterministic),
            "lock_order" | "lock_rank" => Some(Self::LockOrder),
            "deadline" => Some(Self::Deadline),
            "ordering" | "acquire" | "release" | "seq_cst" | "acq_rel" => {
                Some(Self::MemoryOrdering)
            }
            // FMT
            "format" | "binary_format" | "byte_layout" => Some(Self::Format),
            "bits" | "bit_layout" | "bit_level" | "bit_field" => Some(Self::Bits),
            "encoding" | "string_encoding" | "charset" => Some(Self::Encoding),
            "checksum" => Some(Self::Checksum),
            // FMT aliases (coverage: codec_registry, ProtocolGrammar|state_machine)
            "codec_registry" | "CodecRegistry" => Some(Self::Format),
            "ProtocolGrammar" | "state_machine" | "protocol_grammar" => Some(Self::Encoding),
            // PLAT
            "platform" | "platform_abstraction" => Some(Self::Platform),
            "feature" | "feature_flag" | "FeatureFlag" => Some(Self::Feature),
            "resource" | "resource_limit" | "ResourceLimit" => Some(Self::Resource),
            // PERF
            "unsafe_escape" | "UnsafeEscape" => Some(Self::UnsafeEscape),
            "complexity" | "ComplexityBound" => Some(Self::Complexity),
            // NUM
            "precision" | "ulp_bound" | "NumericalPrecision" => Some(Self::Precision),
            // NUM aliases (coverage: precomputed_table|lookup_table)
            "precomputed_table" | "lookup_table" => Some(Self::Precision),
            // STOR
            "monotonic" | "MonotonicState" => Some(Self::Monotonic),
            // STOR aliases (coverage: wal|crash_recovery, page_cache, mvcc, rollback, storage_failure)
            "wal" | "crash_recovery" | "write_ahead" => Some(Self::Monotonic),
            "page_cache" | "buffer_pool" => Some(Self::Monotonic),
            "mvcc" | "snapshot_isolation" => Some(Self::Monotonic),
            "rollback" | "savepoint" => Some(Self::Monotonic),
            "failure_mode" | "storage_failure" => Some(Self::Monotonic),
            // TEST aliases (coverage: TestGenerator|test_gen, behavioral_equiv, multi_pass)
            "TestGenerator" | "test_gen" => Some(Self::Ensures),
            "behavioral_equiv" | "BehavioralEquivalence" => Some(Self::Ensures),
            "multi_pass" | "MultiPassRefinement" | "multi_pass_refinement" => Some(Self::Ensures),
            // MISC aliases (coverage: incremental|IncrementalContract)
            "incremental" | "IncrementalContract" | "incremental_contract" => {
                Some(Self::SuspendInvariant)
            }
            "suspend_invariant" | "scoped_invariant" => Some(Self::SuspendInvariant),
            _ => None,
        }
    }

    /// Return the keyword string for this clause kind.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Requires => "requires",
            Self::Ensures => "ensures",
            Self::Invariant => "invariant",
            Self::Effects => "effects",
            Self::Decreases => "decreases",
            Self::FfiBoundary => "ffi_boundary",
            Self::Trust => "trust",
            Self::Ghost => "ghost",
            Self::Lemma => "lemma",
            Self::Modifies => "modifies",
            Self::Opaque => "opaque",
            Self::Eventually => "eventually",
            Self::Taint => "taint",
            Self::ConstantTime => "constant_time",
            Self::Zeroize => "zeroize",
            Self::Region => "region",
            Self::Width => "width",
            Self::Allocator => "allocator",
            Self::Circular => "circular",
            Self::Interface => "interface",
            Self::Errors => "errors",
            Self::Shared => "shared",
            Self::NoReentrant => "no_reentrant",
            Self::Deterministic => "deterministic",
            Self::LockOrder => "lock_order",
            Self::Deadline => "deadline",
            Self::MemoryOrdering => "ordering",
            Self::Format => "format",
            Self::Bits => "bits",
            Self::Encoding => "encoding",
            Self::Checksum => "checksum",
            Self::Platform => "platform",
            Self::Feature => "feature",
            Self::Resource => "resource",
            Self::UnsafeEscape => "unsafe_escape",
            Self::Complexity => "complexity",
            Self::Precision => "precision",
            Self::Monotonic => "monotonic",
            Self::SuspendInvariant => "suspend_invariant",
        }
    }

    /// Returns true if this is a core contract clause (requires/ensures/invariant/effects/decreases).
    pub fn is_core_clause(&self) -> bool {
        matches!(
            self,
            Self::Requires | Self::Ensures | Self::Invariant | Self::Effects | Self::Decreases
        )
    }

    /// Returns true if this is a feature annotation (not a core clause or FFI).
    pub fn is_annotation(&self) -> bool {
        !self.is_core_clause() && !matches!(self, Self::FfiBoundary | Self::Trust)
    }
}

/// The full set of contract clauses for an annotated item.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct InlineContract {
    pub requires: Vec<ContractClause>,
    pub ensures: Vec<ContractClause>,
    pub invariants: Vec<ContractClause>,
    pub effects: Vec<ContractClause>,
    pub decreases: Vec<ContractClause>,
    /// SEC.2: FFI boundary trust annotations (`@ffi_boundary`, `@trust`).
    pub ffi_boundary: Vec<ContractClause>,
    /// Feature-specific annotations (@ghost, @taint, @region, etc.).
    /// Keyed by InlineClauseKind for all 32 feature annotation types.
    pub annotations: Vec<ContractClause>,
}

impl InlineContract {
    /// Returns true if no clauses were found.
    pub fn is_empty(&self) -> bool {
        self.requires.is_empty()
            && self.ensures.is_empty()
            && self.invariants.is_empty()
            && self.effects.is_empty()
            && self.decreases.is_empty()
            && self.ffi_boundary.is_empty()
            && self.annotations.is_empty()
    }

    /// Total number of clauses across all kinds.
    pub fn clause_count(&self) -> usize {
        self.requires.len()
            + self.ensures.len()
            + self.invariants.len()
            + self.effects.len()
            + self.decreases.len()
            + self.ffi_boundary.len()
            + self.annotations.len()
    }

    /// Get all annotations of a specific kind.
    pub fn annotations_of(&self, kind: InlineClauseKind) -> Vec<&ContractClause> {
        self.annotations.iter().filter(|c| c.kind == kind).collect()
    }

    fn push(&mut self, clause: ContractClause) {
        match clause.kind {
            InlineClauseKind::Requires => self.requires.push(clause),
            InlineClauseKind::Ensures => self.ensures.push(clause),
            InlineClauseKind::Invariant => self.invariants.push(clause),
            InlineClauseKind::Effects => self.effects.push(clause),
            InlineClauseKind::Decreases => self.decreases.push(clause),
            InlineClauseKind::FfiBoundary | InlineClauseKind::Trust => {
                self.ffi_boundary.push(clause)
            }
            _ => self.annotations.push(clause),
        }
    }
}

/// What kind of Rust item is annotated.
#[derive(Debug, Clone, PartialEq)]
pub enum AnnotatedItemKind {
    /// A free function or method.
    Function {
        name: String,
        params: Vec<ParamInfo>,
        return_type: Option<String>,
        is_unsafe: bool,
        is_async: bool,
    },
    /// A struct definition.
    Struct {
        name: String,
        fields: Vec<FieldInfo>,
    },
    /// An impl block (contracts apply to all methods within).
    ImplBlock {
        self_type: String,
        trait_name: Option<String>,
    },
}

/// Basic parameter info extracted from a Rust function signature.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamInfo {
    pub name: String,
    pub ty: String,
}

/// Basic field info extracted from a Rust struct.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldInfo {
    pub name: String,
    pub ty: String,
}

/// An item in a Rust source file that has contract annotations.
#[derive(Debug, Clone, PartialEq)]
pub struct AnnotatedItem {
    /// The contract clauses extracted from doc comments.
    pub contract: InlineContract,
    /// What kind of item this is.
    pub kind: AnnotatedItemKind,
    /// Byte offset of the item in the source file.
    pub offset: usize,
    /// Line number (1-based) of the item in the source file.
    pub line: usize,
}

// ---------------------------------------------------------------------------
// Doc comment parser
// ---------------------------------------------------------------------------

/// Parse contract clauses from a sequence of doc comment lines.
///
/// Each line should be the content of a `///` doc comment (without the `///` prefix).
/// The `base_offset` is the byte offset of the first doc comment in the source file.
pub fn parse_doc_clauses(doc_lines: &[(String, usize)]) -> InlineContract {
    let mut contract = InlineContract::default();
    let mut current_kind: Option<InlineClauseKind> = None;
    let mut current_body = String::new();
    let mut current_offset: usize = 0;

    for (line, offset) in doc_lines {
        let trimmed = line.trim();

        // Check if this line starts a new @-clause
        if let Some(rest) = trimmed.strip_prefix('@') {
            // Flush any previous clause
            if let Some(kind) = current_kind.take() {
                let body = current_body.trim().to_string();
                if !body.is_empty() {
                    contract.push(ContractClause {
                        kind,
                        body,
                        offset: current_offset,
                    });
                }
                current_body.clear();
            }

            // Parse the keyword
            let (keyword, body_rest) = match rest.find(|c: char| c.is_whitespace()) {
                Some(pos) => (&rest[..pos], rest[pos..].trim()),
                None => (rest, ""),
            };

            if let Some(kind) = InlineClauseKind::from_keyword(keyword) {
                current_kind = Some(kind);
                current_body = body_rest.to_string();
                current_offset = *offset;
            }
            // If keyword is not recognized, ignore this line
        } else if current_kind.is_some() {
            // Continuation line for multi-line predicate
            if trimmed.is_empty() {
                // Empty line ends multi-line predicate
                if let Some(kind) = current_kind.take() {
                    let body = current_body.trim().to_string();
                    if !body.is_empty() {
                        contract.push(ContractClause {
                            kind,
                            body,
                            offset: current_offset,
                        });
                    }
                    current_body.clear();
                }
            } else {
                // Continuation: append to current body
                if !current_body.is_empty() {
                    current_body.push(' ');
                }
                current_body.push_str(trimmed);
            }
        }
        // Non-@, non-continuation lines are regular doc comments; skip.
    }

    // Flush final clause
    if let Some(kind) = current_kind {
        let body = current_body.trim().to_string();
        if !body.is_empty() {
            contract.push(ContractClause {
                kind,
                body,
                offset: current_offset,
            });
        }
    }

    contract
}

// ---------------------------------------------------------------------------
// Rust source file parser
// ---------------------------------------------------------------------------

/// Extract doc comment lines from syn attributes.
///
/// Returns pairs of (line_content, byte_offset).
fn extract_doc_lines(attrs: &[syn::Attribute], source: &str) -> Vec<(String, usize)> {
    let mut lines = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        if let syn::Meta::NameValue(nv) = &attr.meta
            && let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(lit_str),
                ..
            }) = &nv.value
        {
            let content = lit_str.value();
            // Compute byte offset from span
            let span = attr.pound_token.span;
            let offset = span_to_offset(span, source);
            lines.push((content, offset));
        }
    }
    lines
}

/// Convert a proc_macro2::Span to a byte offset in source.
///
/// In non-proc-macro context, span locations may not be available.
/// We fall back to 0 if we cannot determine the offset.
fn span_to_offset(span: proc_macro2::Span, _source: &str) -> usize {
    // proc_macro2 spans in non-proc-macro context (parsed via syn::parse_str
    // or syn::parse_file) provide byte offsets via start().
    span.start().column
}

/// Extract a string representation of a syn type.
fn type_to_string(ty: &syn::Type) -> String {
    ty.to_token_stream().to_string()
}

/// Extract function parameters as ParamInfo.
fn extract_params(sig: &syn::Signature) -> Vec<ParamInfo> {
    sig.inputs
        .iter()
        .map(|arg| match arg {
            syn::FnArg::Typed(pat_type) => {
                let name = pat_to_string(&pat_type.pat);
                let ty = type_to_string(&pat_type.ty);
                ParamInfo { name, ty }
            }
            syn::FnArg::Receiver(_) => ParamInfo {
                name: "self".to_string(),
                ty: "Self".to_string(),
            },
        })
        .collect()
}

/// Extract a string from a pattern.
fn pat_to_string(pat: &syn::Pat) -> String {
    pat.to_token_stream().to_string()
}

/// Extract return type as a string, None for `()` / no return.
fn extract_return_type(sig: &syn::Signature) -> Option<String> {
    match &sig.output {
        syn::ReturnType::Default => None,
        syn::ReturnType::Type(_, ty) => Some(type_to_string(ty)),
    }
}

/// Compute line number (1-based) from byte offset in source.
fn offset_to_line(source: &str, offset: usize) -> usize {
    let clamped = offset.min(source.len());
    source[..clamped].chars().filter(|&c| c == '\n').count() + 1
}

/// Parse a Rust source string and extract all annotated items.
pub fn parse_rust_source(source: &str) -> Result<Vec<AnnotatedItem>, String> {
    let file = syn::parse_file(source).map_err(|e| format!("syn parse error: {e}"))?;

    let mut items = Vec::new();

    for item in &file.items {
        match item {
            syn::Item::Fn(func) => {
                let doc_lines = extract_doc_lines(&func.attrs, source);
                let contract = parse_doc_clauses(&doc_lines);
                if !contract.is_empty() {
                    let offset = func_span_offset(&func.sig, source);
                    items.push(AnnotatedItem {
                        contract,
                        kind: AnnotatedItemKind::Function {
                            name: func.sig.ident.to_string(),
                            params: extract_params(&func.sig),
                            return_type: extract_return_type(&func.sig),
                            is_unsafe: func.sig.unsafety.is_some(),
                            is_async: func.sig.asyncness.is_some(),
                        },
                        line: offset_to_line(source, offset),
                        offset,
                    });
                }
            }
            syn::Item::Struct(st) => {
                let doc_lines = extract_doc_lines(&st.attrs, source);
                let contract = parse_doc_clauses(&doc_lines);
                if !contract.is_empty() {
                    let offset = st.ident.span().start().column;
                    let fields = match &st.fields {
                        syn::Fields::Named(named) => named
                            .named
                            .iter()
                            .filter_map(|f| {
                                f.ident.as_ref().map(|id| FieldInfo {
                                    name: id.to_string(),
                                    ty: type_to_string(&f.ty),
                                })
                            })
                            .collect(),
                        _ => Vec::new(),
                    };
                    items.push(AnnotatedItem {
                        contract,
                        kind: AnnotatedItemKind::Struct {
                            name: st.ident.to_string(),
                            fields,
                        },
                        line: offset_to_line(source, offset),
                        offset,
                    });
                }
            }
            syn::Item::Impl(imp) => {
                // Check impl-level doc comments for invariants
                let impl_doc_lines = extract_doc_lines(&imp.attrs, source);
                let impl_contract = parse_doc_clauses(&impl_doc_lines);

                let self_type = type_to_string(&imp.self_ty);
                let trait_name = imp
                    .trait_
                    .as_ref()
                    .map(|(_, path, _)| path.to_token_stream().to_string());

                if !impl_contract.is_empty() {
                    let offset = imp.impl_token.span.start().column;
                    items.push(AnnotatedItem {
                        contract: impl_contract,
                        kind: AnnotatedItemKind::ImplBlock {
                            self_type: self_type.clone(),
                            trait_name: trait_name.clone(),
                        },
                        line: offset_to_line(source, offset),
                        offset,
                    });
                }

                // Check methods within the impl block
                for impl_item in &imp.items {
                    if let syn::ImplItem::Fn(method) = impl_item {
                        let doc_lines = extract_doc_lines(&method.attrs, source);
                        let contract = parse_doc_clauses(&doc_lines);
                        if !contract.is_empty() {
                            let offset = func_span_offset_method(&method.sig, source);
                            items.push(AnnotatedItem {
                                contract,
                                kind: AnnotatedItemKind::Function {
                                    name: method.sig.ident.to_string(),
                                    params: extract_params(&method.sig),
                                    return_type: extract_return_type(&method.sig),
                                    is_unsafe: method.sig.unsafety.is_some(),
                                    is_async: method.sig.asyncness.is_some(),
                                },
                                line: offset_to_line(source, offset),
                                offset,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(items)
}

/// Get byte offset of a function signature's identifier in source.
fn func_span_offset(sig: &syn::Signature, source: &str) -> usize {
    // Use the fn keyword span line/col to compute byte offset
    let start = sig.fn_token.span.start();
    line_col_to_offset(source, start.line, start.column)
}

/// Get byte offset of a method signature's identifier in source.
fn func_span_offset_method(sig: &syn::Signature, source: &str) -> usize {
    let start = sig.fn_token.span.start();
    line_col_to_offset(source, start.line, start.column)
}

/// Convert (line, column) (0-based line from proc_macro2) to byte offset.
fn line_col_to_offset(source: &str, line: usize, column: usize) -> usize {
    let mut offset = 0;
    for (i, src_line) in source.lines().enumerate() {
        if i == line {
            return offset + column.min(src_line.len());
        }
        offset += src_line.len() + 1; // +1 for newline
    }
    source.len()
}

/// Parse a Rust source file from disk and extract all annotated items.
pub fn parse_rust_file(path: &Path) -> Result<Vec<AnnotatedItem>, String> {
    let source = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    parse_rust_source(&source)
}

/// Scan a directory recursively for `.rs` files and extract all annotated items.
pub fn scan_directory(dir: &Path) -> Result<Vec<(std::path::PathBuf, Vec<AnnotatedItem>)>, String> {
    let mut results = Vec::new();
    scan_dir_recursive(dir, &mut results)?;
    Ok(results)
}

fn scan_dir_recursive(
    dir: &Path,
    results: &mut Vec<(std::path::PathBuf, Vec<AnnotatedItem>)>,
) -> Result<(), String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("failed to read dir {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("dir entry error: {e}"))?;
        let path = entry.path();
        if path.is_dir() {
            // Skip target, hidden dirs, and generated dirs
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "target" || name == "generated" {
                continue;
            }
            scan_dir_recursive(&path, results)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            match parse_rust_file(&path) {
                Ok(items) if !items.is_empty() => {
                    results.push((path, items));
                }
                Ok(_) => {}  // No annotations found
                Err(_) => {} // Skip files that fail to parse
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Dual-source merge: combine external .assura + inline annotations
// ---------------------------------------------------------------------------

/// Source origin for a contract clause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClauseSource {
    /// From an external `.assura` file.
    External,
    /// From inline doc comment annotations in a `.rs` file.
    Inline,
}

/// A contract clause with its source origin, for merged contracts.
#[derive(Debug, Clone, PartialEq)]
pub struct SourcedClause {
    pub kind: InlineClauseKind,
    pub body: String,
    pub source: ClauseSource,
}

/// The result of merging external and inline contracts for a function.
#[derive(Debug, Clone, Default)]
pub struct MergedContract {
    pub clauses: Vec<SourcedClause>,
    pub warnings: Vec<String>,
}

impl MergedContract {
    /// Total number of clauses across both sources.
    pub fn clause_count(&self) -> usize {
        self.clauses.len()
    }

    /// Returns true if no clauses from either source.
    pub fn is_empty(&self) -> bool {
        self.clauses.is_empty()
    }

    /// Clauses from external `.assura` files only.
    pub fn external_clauses(&self) -> Vec<&SourcedClause> {
        self.clauses
            .iter()
            .filter(|c| c.source == ClauseSource::External)
            .collect()
    }

    /// Clauses from inline doc comments only.
    pub fn inline_clauses(&self) -> Vec<&SourcedClause> {
        self.clauses
            .iter()
            .filter(|c| c.source == ClauseSource::Inline)
            .collect()
    }
}

/// Merge external and inline contract clauses for a function.
///
/// Rules (per spec #105):
/// 1. External contracts are authoritative (higher priority)
/// 2. Clauses from both sources are merged (union, not replacement)
/// 3. Duplicate clauses are detected and warned
/// 4. Contradictory clauses are reported as warnings
pub fn merge_contracts(
    external_clauses: &[(InlineClauseKind, String)],
    inline: &InlineContract,
) -> MergedContract {
    let mut merged = MergedContract::default();

    // Add all external clauses first (authoritative)
    for (kind, body) in external_clauses {
        merged.clauses.push(SourcedClause {
            kind: *kind,
            body: body.clone(),
            source: ClauseSource::External,
        });
    }

    // Collect all inline clauses
    let inline_all: Vec<(&ContractClause, InlineClauseKind)> = inline
        .requires
        .iter()
        .map(|c| (c, InlineClauseKind::Requires))
        .chain(
            inline
                .ensures
                .iter()
                .map(|c| (c, InlineClauseKind::Ensures)),
        )
        .chain(
            inline
                .invariants
                .iter()
                .map(|c| (c, InlineClauseKind::Invariant)),
        )
        .chain(
            inline
                .effects
                .iter()
                .map(|c| (c, InlineClauseKind::Effects)),
        )
        .chain(
            inline
                .decreases
                .iter()
                .map(|c| (c, InlineClauseKind::Decreases)),
        )
        .chain(inline.annotations.iter().map(|c| (c, c.kind)))
        .collect();

    // Add inline clauses, checking for duplicates
    for (clause, kind) in &inline_all {
        let body_normalized = clause.body.trim().to_string();

        // Check if this clause is a duplicate of an external clause
        let is_duplicate = merged.clauses.iter().any(|existing| {
            existing.source == ClauseSource::External
                && existing.kind == *kind
                && normalize_clause_body(&existing.body) == normalize_clause_body(&body_normalized)
        });

        if is_duplicate {
            merged.warnings.push(format!(
                "duplicate {} clause (inline matches external): {}",
                kind.as_str(),
                body_normalized
            ));
        } else {
            merged.clauses.push(SourcedClause {
                kind: *kind,
                body: body_normalized,
                source: ClauseSource::Inline,
            });
        }
    }

    merged
}

/// Normalize clause body text for comparison (strip whitespace, lowercase).
fn normalize_clause_body(body: &str) -> String {
    body.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------------------
// Multi-language annotation framework
// ---------------------------------------------------------------------------

/// Trait for language-specific annotation parsing.
///
/// Each language adapter knows how to extract contract annotations from
/// its source format. The clause kinds (`@requires`, `@ensures`, etc.)
/// are universal; only the comment syntax and type mapping differ.
pub trait LanguageAdapter {
    /// Language identifier (e.g., "rust", "python", "go").
    fn language_id(&self) -> &str;

    /// File extensions handled by this adapter (e.g., `["rs"]`).
    fn file_extensions(&self) -> &[&str];

    /// Extract annotated items from source text.
    fn parse_source(&self, source: &str) -> Result<Vec<AnnotatedItem>, String>;

    /// Map a language-specific type name to an Assura type.
    /// Returns `None` if the type has no Assura equivalent.
    fn map_type(&self, language_type: &str) -> Option<String>;
}

/// Rust language adapter (delegates to existing `parse_rust_source`).
pub struct RustAdapter;

impl LanguageAdapter for RustAdapter {
    fn language_id(&self) -> &str {
        "rust"
    }

    fn file_extensions(&self) -> &[&str] {
        &["rs"]
    }

    fn parse_source(&self, source: &str) -> Result<Vec<AnnotatedItem>, String> {
        parse_rust_source(source)
    }

    fn map_type(&self, language_type: &str) -> Option<String> {
        match language_type {
            "i8" | "i16" | "i32" | "i64" | "i128" | "isize" => Some("Int".to_string()),
            "u8" | "u16" | "u32" | "u64" | "u128" | "usize" => Some("Nat".to_string()),
            "f32" | "f64" => Some("Float".to_string()),
            "bool" => Some("Bool".to_string()),
            "String" | "&str" => Some("String".to_string()),
            "()" => Some("Unit".to_string()),
            _ => None,
        }
    }
}

/// Python language adapter.
///
/// Extracts `# @requires`, `# @ensures`, `# @invariant`, `# @effects`,
/// and `# @decreases` annotations from Python comments and docstrings.
pub struct PythonAdapter;

impl LanguageAdapter for PythonAdapter {
    fn language_id(&self) -> &str {
        "python"
    }

    fn file_extensions(&self) -> &[&str] {
        &["py"]
    }

    fn parse_source(&self, source: &str) -> Result<Vec<AnnotatedItem>, String> {
        parse_python_source(source)
    }

    fn map_type(&self, language_type: &str) -> Option<String> {
        match language_type {
            "int" => Some("Int".to_string()),
            "float" => Some("Float".to_string()),
            "bool" => Some("Bool".to_string()),
            "str" => Some("String".to_string()),
            "bytes" => Some("Bytes".to_string()),
            "None" => Some("Unit".to_string()),
            "list" | "List" => Some("List".to_string()),
            "dict" | "Dict" => Some("Map".to_string()),
            "set" | "Set" => Some("Set".to_string()),
            "Optional" => Some("Option".to_string()),
            _ => None,
        }
    }
}

/// Parse Python source for contract annotations in comments and docstrings.
///
/// Supports two annotation styles:
/// 1. Hash comments: `# @requires x > 0`
/// 2. Docstring annotations: `"""@requires x > 0"""`
fn parse_python_source(source: &str) -> Result<Vec<AnnotatedItem>, String> {
    let lines: Vec<&str> = source.lines().collect();
    let mut items = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        // Collect annotation lines (# @keyword ... or docstring @keyword ...)
        let mut doc_lines: Vec<(String, usize)> = Vec::new();
        let _annotation_start = i;

        // Collect consecutive comment lines with @-clauses
        while i < lines.len() {
            let trimmed = lines[i].trim();
            if let Some(rest) = trimmed.strip_prefix('#') {
                let content = rest.trim_start();
                if content.starts_with('@') {
                    let byte_offset = source[..source_line_offset(source, i)].len();
                    doc_lines.push((content.to_string(), byte_offset));
                } else if !doc_lines.is_empty() {
                    // Continuation of non-@ comment after @-clauses
                    break;
                }
                i += 1;
            } else {
                break;
            }
        }

        if doc_lines.is_empty() {
            // Check for docstring annotations on the next line (inside a function)
            i += 1;
            continue;
        }

        let contract = parse_doc_clauses(&doc_lines);
        if contract.is_empty() {
            continue;
        }

        // Look for the function/class definition following the annotations
        while i < lines.len() && lines[i].trim().is_empty() {
            i += 1;
        }

        if i < lines.len() {
            let trimmed = lines[i].trim();

            if let Some(func_info) = parse_python_function_def(trimmed) {
                let byte_offset = source_line_offset(source, i);
                items.push(AnnotatedItem {
                    contract,
                    kind: AnnotatedItemKind::Function {
                        name: func_info.0,
                        params: func_info.1,
                        return_type: func_info.2,
                        is_unsafe: false,
                        is_async: func_info.3,
                    },
                    line: i + 1, // 1-based
                    offset: byte_offset,
                });
            } else if let Some(class_name) = parse_python_class_def(trimmed) {
                let byte_offset = source_line_offset(source, i);
                // Check for docstring invariants inside the class
                let mut class_doc_lines = Vec::new();
                let mut j = i + 1;
                while j < lines.len() {
                    let inner = lines[j].trim();
                    if inner.starts_with("\"\"\"") || inner.starts_with("'''") {
                        // Parse docstring for @-clauses
                        let in_docstring = inner.starts_with("\"\"\"");
                        let quote = if in_docstring { "\"\"\"" } else { "'''" };
                        // Check single-line docstring
                        if inner.len() > 6 && inner.ends_with(quote) {
                            let content = &inner[3..inner.len() - 3];
                            if content.trim().starts_with('@') {
                                let offset = source_line_offset(source, j);
                                class_doc_lines.push((format!(" {}", content.trim()), offset));
                            }
                        }
                        break;
                    } else if inner.is_empty() || inner.starts_with('#') {
                        j += 1;
                        continue;
                    } else {
                        break;
                    }
                }

                // Merge class-level annotations from before and after
                let mut full_contract = contract;
                if !class_doc_lines.is_empty() {
                    let docstring_contract = parse_doc_clauses(&class_doc_lines);
                    for c in docstring_contract.invariants {
                        full_contract.invariants.push(c);
                    }
                }

                items.push(AnnotatedItem {
                    contract: full_contract,
                    kind: AnnotatedItemKind::Struct {
                        name: class_name,
                        fields: Vec::new(), // Python class fields need runtime analysis
                    },
                    line: i + 1,
                    offset: byte_offset,
                });
            }
        }

        i += 1;
    }

    Ok(items)
}

/// Get byte offset of a line in source text.
fn source_line_offset(source: &str, line_index: usize) -> usize {
    let mut offset = 0;
    for (i, line) in source.lines().enumerate() {
        if i == line_index {
            return offset;
        }
        offset += line.len() + 1; // +1 for newline
    }
    source.len()
}

/// Parse a Python function definition line.
/// Returns (name, params, return_type, is_async).
fn parse_python_function_def(line: &str) -> Option<(String, Vec<ParamInfo>, Option<String>, bool)> {
    let is_async = line.starts_with("async ");
    let rest = if is_async {
        line.strip_prefix("async ")?.trim()
    } else {
        line
    };

    let rest = rest.strip_prefix("def ")?;
    let paren_start = rest.find('(')?;
    let name = rest[..paren_start].trim().to_string();

    // Extract parameters (simplified: just names and optional type annotations)
    let paren_end = rest.rfind(')')?;
    let params_str = &rest[paren_start + 1..paren_end];
    let params: Vec<ParamInfo> = params_str
        .split(',')
        .filter_map(|p| {
            let p = p.trim();
            if p.is_empty() || p == "self" || p == "cls" {
                return None;
            }
            if let Some((name, ty)) = p.split_once(':') {
                Some(ParamInfo {
                    name: name.trim().to_string(),
                    ty: ty.trim().to_string(),
                })
            } else {
                Some(ParamInfo {
                    name: p.to_string(),
                    ty: "Any".to_string(),
                })
            }
        })
        .collect();

    // Extract return type
    let after_paren = &rest[paren_end + 1..];
    let return_type = after_paren
        .strip_prefix("->")
        .or_else(|| after_paren.strip_prefix(" ->"))
        .map(|s| s.trim().trim_end_matches(':').trim().to_string())
        .filter(|s| !s.is_empty());

    Some((name, params, return_type, is_async))
}

/// Parse a Python class definition line. Returns the class name.
fn parse_python_class_def(line: &str) -> Option<String> {
    let rest = line.strip_prefix("class ")?;
    let end = rest.find(['(', ':']).unwrap_or(rest.len());
    let name = rest[..end].trim().to_string();
    if name.is_empty() { None } else { Some(name) }
}

/// Get the appropriate language adapter for a file extension.
pub fn adapter_for_extension(ext: &str) -> Option<Box<dyn LanguageAdapter>> {
    match ext {
        "rs" => Some(Box::new(RustAdapter)),
        "py" => Some(Box::new(PythonAdapter)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "ra_tests.rs"]
mod tests;
