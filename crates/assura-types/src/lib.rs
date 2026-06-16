//! Type checking for the Assura contract language.
//!
//! Builds a `TypeEnv` (type environment) from a `ResolvedFile` by mapping
//! each symbol in the symbol table to its `Type`. For T013 this creates the
//! scaffolding: type environment construction and the `type_check` entry
//! point. Actual expression-level type checking (T014-T018) builds on this.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use assura_parser::ast::{BinOp, BlockKind, ClauseKind, Decl, Expr, ServiceItem};
use assura_resolve::{ImportStatus, ResolvedFile, SymbolKind, SymbolTable};

pub mod checkers;
pub mod clauses;
pub mod domain;
pub mod inference;
use checkers::*;
pub use checkers::{FrameChecker, PendingDecreaseCheck, TaintLabel};
use clauses::*;
use domain::*;
pub use domain::{GeneratedTest, TestGenerator, TestKind, TestableContract};
pub(crate) use inference::*;

// ---------------------------------------------------------------------------
// Type representation
// ---------------------------------------------------------------------------

/// Represents all Assura types.
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    // --- Base types ---
    Int,
    Nat,
    Float,
    Bool,
    String,
    Bytes,
    Unit,
    Never,

    // --- Fixed-width integers ---
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,

    // --- Generic container types ---
    List(Box<Type>),
    Map(Box<Type>, Box<Type>),
    Set(Box<Type>),
    Option(Box<Type>),
    Result(Box<Type>, Box<Type>),

    // --- Sequence (used in demos) ---
    Sequence(Box<Type>),

    // --- User-defined named type ---
    Named(std::string::String),

    // --- Generic type parameter ---
    TypeParam(std::string::String),

    // --- Function type ---
    Fn {
        params: Vec<Type>,
        ret: Box<Type>,
    },

    // --- Tuple type ---
    Tuple(Vec<Type>),

    // --- Refined type: base type with predicate ---
    Refined {
        base: Box<Type>,
        predicate: std::string::String,
    },

    // --- Genuinely unknown type (unresolved reference, unparsed tokens) ---
    Unknown,

    // --- Error recovery: a type error was already reported upstream ---
    /// Distinct from `Unknown`: `Error` suppresses cascading errors,
    /// while `Unknown` means "we genuinely don't know yet".
    Error,
}

impl Type {
    /// Returns `true` if this type is indeterminate (either genuinely
    /// unknown or an error-recovery placeholder). Use this instead of
    /// matching `Type::Unknown` directly when deciding whether to
    /// suppress further diagnostics.
    pub(crate) fn is_indeterminate(&self) -> bool {
        matches!(self, Type::Unknown | Type::Error)
    }
}

// ---------------------------------------------------------------------------
// Type environment
// ---------------------------------------------------------------------------

/// Maps names to their types. This is the typing context built during
/// type checking.
#[derive(Debug, Clone, Default)]
pub struct TypeEnv {
    /// Maps symbol name -> Type.
    pub bindings: HashMap<std::string::String, Type>,
    /// Maps struct type name -> { field_name -> field_type }.
    pub struct_fields: HashMap<std::string::String, Vec<(std::string::String, Type)>>,
}

impl TypeEnv {
    /// Create an empty type environment.
    pub fn new() -> Self {
        Self {
            bindings: HashMap::new(),
            struct_fields: HashMap::new(),
        }
    }

    /// Insert a binding. Returns the previous type if the name was already
    /// bound.
    pub fn insert(&mut self, name: std::string::String, ty: Type) -> Option<Type> {
        self.bindings.insert(name, ty)
    }

    /// Look up a name in the environment.
    pub fn lookup(&self, name: &str) -> Option<&Type> {
        self.bindings.get(name)
    }

    /// Look up a field type on a struct type.
    pub(crate) fn lookup_field(&self, struct_name: &str, field_name: &str) -> Option<&Type> {
        self.struct_fields
            .get(struct_name)
            .and_then(|fields| fields.iter().find(|(n, _)| n == field_name).map(|(_, t)| t))
    }

    /// Number of bindings.
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// Returns true if the environment has no bindings.
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Type errors
// ---------------------------------------------------------------------------

/// A structured type error with error code, span, and message.
#[derive(Debug, Clone)]
pub struct TypeError {
    /// Error code from the spec (A03xxx series).
    pub code: assura_diagnostics::ErrorCode,
    /// Human-readable error message.
    pub message: std::string::String,
    /// Source location where the error was detected.
    pub span: Range<usize>,
    /// Optional secondary span with label (e.g., "expected type declared here").
    pub secondary: Option<(Range<usize>, std::string::String)>,
}

impl From<TypeError> for assura_diagnostics::Diagnostic {
    fn from(e: TypeError) -> Self {
        let mut d = assura_diagnostics::Diagnostic::error(e.code, e.message, e.span);
        if let Some((span, label)) = e.secondary {
            d.secondary.push(assura_diagnostics::SecondaryLabel {
                span,
                message: label,
            });
        }
        d
    }
}

// ---------------------------------------------------------------------------
// Typed file
// ---------------------------------------------------------------------------

/// The result of successful type checking: the resolved file plus the
/// type environment constructed from its symbols.
#[derive(Debug, Clone)]
pub struct TypedFile {
    pub resolved: Arc<ResolvedFile>,
    pub type_env: TypeEnv,
    /// Pending decrease checks that need SMT verification.
    /// The CLI pipeline dispatches these to assura-smt::verify_decrease().
    pub pending_decrease_checks: Vec<PendingDecreaseCheck>,
    /// The HIR file, if available (set when type checking through the HIR path).
    pub hir: Option<assura_hir::HirFile>,
}

// ---------------------------------------------------------------------------
// Built-in type mapping
// ---------------------------------------------------------------------------

/// Map a built-in type name to its `Type` representation.
fn builtin_type(name: &str) -> Option<Type> {
    match name {
        "Int" => Some(Type::Int),
        "Nat" => Some(Type::Nat),
        "Float" => Some(Type::Float),
        "Bool" => Some(Type::Bool),
        "String" => Some(Type::String),
        "Bytes" => Some(Type::Bytes),
        "Unit" => Some(Type::Unit),
        "Never" => Some(Type::Never),
        "U8" => Some(Type::U8),
        "U16" => Some(Type::U16),
        "U32" => Some(Type::U32),
        "U64" => Some(Type::U64),
        "I8" => Some(Type::I8),
        "I16" => Some(Type::I16),
        "I32" => Some(Type::I32),
        "I64" => Some(Type::I64),
        "F32" => Some(Type::F32),
        "F64" => Some(Type::F64),
        // Generic container types with no type arguments (bare names).
        // Full `List<Int>` etc. is handled by parse_type_tokens above.
        "List" => Some(Type::List(Box::new(Type::Unknown))),
        "Map" => Some(Type::Map(Box::new(Type::Unknown), Box::new(Type::Unknown))),
        "Set" => Some(Type::Set(Box::new(Type::Unknown))),
        "Option" => Some(Type::Option(Box::new(Type::Unknown))),
        "Result" => Some(Type::Result(
            Box::new(Type::Unknown),
            Box::new(Type::Unknown),
        )),
        "Sequence" => Some(Type::Sequence(Box::new(Type::Unknown))),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// TypeExpr -> Type conversion
// ---------------------------------------------------------------------------

/// Convert a structured `TypeExpr` (from the parser) to a type-checker `Type`.
///
/// This provides a cleaner, faster path than re-parsing raw tokens.
pub(crate) fn type_from_expr(expr: &assura_parser::ast::TypeExpr) -> Type {
    use assura_parser::ast::TypeExpr;
    match expr {
        TypeExpr::Unit => Type::Unit,
        TypeExpr::Named(name) => builtin_type(name).unwrap_or_else(|| Type::Named(name.clone())),
        TypeExpr::Generic(name, args) => {
            let type_args: Vec<Type> = args.iter().map(type_from_expr).collect();
            match name.as_str() {
                "List" | "Vec" => Type::List(Box::new(
                    type_args.into_iter().next().unwrap_or(Type::Unknown),
                )),
                "Sequence" => Type::Sequence(Box::new(
                    type_args.into_iter().next().unwrap_or(Type::Unknown),
                )),
                "Set" => Type::Set(Box::new(
                    type_args.into_iter().next().unwrap_or(Type::Unknown),
                )),
                "Option" => Type::Option(Box::new(
                    type_args.into_iter().next().unwrap_or(Type::Unknown),
                )),
                "Map" => {
                    let mut it = type_args.into_iter();
                    Type::Map(
                        Box::new(it.next().unwrap_or(Type::Unknown)),
                        Box::new(it.next().unwrap_or(Type::Unknown)),
                    )
                }
                "Result" => {
                    let mut it = type_args.into_iter();
                    Type::Result(
                        Box::new(it.next().unwrap_or(Type::Unknown)),
                        Box::new(it.next().unwrap_or(Type::Unknown)),
                    )
                }
                _ => Type::Named(name.clone()),
            }
        }
        TypeExpr::Tuple(elems) => Type::Tuple(elems.iter().map(type_from_expr).collect()),
        TypeExpr::Fn { params, ret } => Type::Fn {
            params: params.iter().map(type_from_expr).collect(),
            ret: Box::new(type_from_expr(ret)),
        },
        TypeExpr::Refined { base, predicate } => Type::Refined {
            base: Box::new(type_from_expr(base)),
            predicate: predicate.clone(),
        },
    }
}

/// Try to resolve a type from a parsed_type first, falling back to raw token parsing.
pub(crate) fn resolve_type(
    parsed_type: Option<&assura_parser::ast::TypeExpr>,
    tokens: &[String],
) -> Type {
    if let Some(te) = parsed_type {
        type_from_expr(te)
    } else {
        parse_type_tokens(tokens)
    }
}

/// Convert an `HirType` to the type checker's `Type`.
pub(crate) fn type_from_hir_type(hir_ty: &assura_hir::HirType) -> Type {
    use assura_hir::HirType;
    match hir_ty {
        HirType::Unit => Type::Unit,
        HirType::Named(name) => builtin_type(name).unwrap_or_else(|| Type::Named(name.clone())),
        HirType::Generic(name, args) => {
            let type_args: Vec<Type> = args.iter().map(type_from_hir_type).collect();
            match name.as_str() {
                "List" | "Vec" => Type::List(Box::new(
                    type_args.into_iter().next().unwrap_or(Type::Unknown),
                )),
                "Sequence" => Type::Sequence(Box::new(
                    type_args.into_iter().next().unwrap_or(Type::Unknown),
                )),
                "Set" => Type::Set(Box::new(
                    type_args.into_iter().next().unwrap_or(Type::Unknown),
                )),
                "Option" => Type::Option(Box::new(
                    type_args.into_iter().next().unwrap_or(Type::Unknown),
                )),
                "Map" => {
                    let mut it = type_args.into_iter();
                    Type::Map(
                        Box::new(it.next().unwrap_or(Type::Unknown)),
                        Box::new(it.next().unwrap_or(Type::Unknown)),
                    )
                }
                "Result" => {
                    let mut it = type_args.into_iter();
                    Type::Result(
                        Box::new(it.next().unwrap_or(Type::Unknown)),
                        Box::new(it.next().unwrap_or(Type::Unknown)),
                    )
                }
                _ => Type::Named(name.clone()),
            }
        }
        HirType::Tuple(elems) => Type::Tuple(elems.iter().map(type_from_hir_type).collect()),
        HirType::Fn { params, ret } => Type::Fn {
            params: params.iter().map(type_from_hir_type).collect(),
            ret: Box::new(type_from_hir_type(ret)),
        },
        HirType::Refined { base, predicate } => Type::Refined {
            base: Box::new(type_from_hir_type(base)),
            predicate: predicate.clone(),
        },
        HirType::Unresolved(tokens) => parse_type_tokens(tokens),
    }
}

// ---------------------------------------------------------------------------
// Type token parsing
// ---------------------------------------------------------------------------

/// Parse a raw token sequence (e.g. `["List", "<", "Int", ">"]`) into a
/// structured `Type`. Handles base types, generic containers, refinement
/// types, taint annotations, reference/mutable types, and union error types.
pub(crate) fn parse_type_tokens(tokens: &[String]) -> Type {
    if tokens.is_empty() {
        return Type::Unit;
    }

    // Strip taint annotations (everything from "@" onward)
    let clean: Vec<&str> = tokens
        .iter()
        .map(|s| s.as_str())
        .take_while(|t| *t != "@")
        .collect();
    if clean.is_empty() {
        return Type::Unit;
    }

    // Strip leading & or &mut (references)
    let clean = if clean.first() == Some(&"&") {
        if clean.get(1) == Some(&"mut") {
            &clean[2..]
        } else {
            &clean[1..]
        }
    } else {
        &clean[..]
    };
    if clean.is_empty() {
        return Type::Unknown;
    }

    // Refinement type: { x : T | P }
    if clean.first() == Some(&"{") {
        // Find the colon to extract the base type
        if let Some(colon_pos) = clean.iter().position(|t| *t == ":") {
            let after_colon: Vec<&str> = clean[colon_pos + 1..]
                .iter()
                .take_while(|t| **t != "|" && **t != "}")
                .copied()
                .collect();
            let owned: Vec<String> = after_colon.iter().map(|s| s.to_string()).collect();
            let base = parse_type_tokens(&owned);

            // Extract predicate: everything between | and }
            let predicate = if let Some(pipe_pos) = clean.iter().position(|t| *t == "|") {
                clean[pipe_pos + 1..]
                    .iter()
                    .take_while(|t| **t != "}")
                    .copied()
                    .collect::<Vec<&str>>()
                    .join(" ")
            } else {
                std::string::String::new()
            };

            return Type::Refined {
                base: Box::new(base),
                predicate,
            };
        }
        return Type::Unknown;
    }

    // Handle union error types: T | E -> Result<T, E> at top level
    let mut depth = 0i32;
    let mut pipe_pos = None;
    for (i, tok) in clean.iter().enumerate() {
        match *tok {
            "<" => depth += 1,
            ">" if depth > 0 => depth -= 1,
            "|" if depth == 0 => {
                pipe_pos = Some(i);
                break;
            }
            _ => {}
        }
    }
    if let Some(pp) = pipe_pos {
        let ok_tokens: Vec<String> = clean[..pp].iter().map(|s| s.to_string()).collect();
        let err_tokens: Vec<String> = clean[pp + 1..].iter().map(|s| s.to_string()).collect();
        let ok_ty = parse_type_tokens(&ok_tokens);
        let err_ty = parse_type_tokens(&err_tokens);
        return Type::Result(Box::new(ok_ty), Box::new(err_ty));
    }

    let head = clean[0];

    // Function type: fn ( A , B ) -> C
    if head == "fn" && clean.len() >= 3 && clean[1] == "(" {
        // Find matching closing paren
        let mut depth = 0i32;
        let mut close_paren = None;
        for (i, tok) in clean[1..].iter().enumerate() {
            match *tok {
                "(" => depth += 1,
                ")" => {
                    depth -= 1;
                    if depth == 0 {
                        close_paren = Some(i + 1); // offset by 1 for the slice
                        break;
                    }
                }
                _ => {}
            }
        }
        if let Some(cp) = close_paren {
            // Parse parameter types from between ( and )
            let param_tokens = &clean[2..cp];
            let mut params: Vec<Type> = Vec::new();
            let mut current: Vec<String> = Vec::new();
            let mut d = 0i32;
            for tok in param_tokens {
                match *tok {
                    "(" | "<" => {
                        d += 1;
                        current.push(tok.to_string());
                    }
                    ")" | ">" => {
                        d -= 1;
                        current.push(tok.to_string());
                    }
                    "," if d == 0 => {
                        if !current.is_empty() {
                            params.push(parse_type_tokens(&current));
                            current.clear();
                        }
                    }
                    _ => current.push(tok.to_string()),
                }
            }
            if !current.is_empty() {
                params.push(parse_type_tokens(&current));
            }

            // Check for -> return type after the closing paren
            let after_paren = &clean[cp + 1..];
            let ret = if after_paren.len() >= 2 && after_paren[0] == "->" {
                let ret_tokens: Vec<String> =
                    after_paren[1..].iter().map(|s| s.to_string()).collect();
                Box::new(parse_type_tokens(&ret_tokens))
            } else {
                Box::new(Type::Unit)
            };

            return Type::Fn { params, ret };
        }
    }

    // Single-token base types
    if clean.len() == 1 {
        if let Some(ty) = builtin_type(head) {
            return ty;
        }
        return Type::Named(head.to_string());
    }

    // Generic container: Name < Args... >
    if clean.len() >= 3 && clean[1] == "<" {
        // Collect type arguments between < and >
        let inner = &clean[2..];
        // Strip trailing >
        let inner = if inner.last() == Some(&">") {
            &inner[..inner.len() - 1]
        } else {
            inner
        };

        // Split on commas at depth 0
        let mut args: Vec<Type> = Vec::new();
        let mut current: Vec<String> = Vec::new();
        let mut d = 0i32;
        for tok in inner {
            match *tok {
                "<" => {
                    d += 1;
                    current.push(tok.to_string());
                }
                ">" => {
                    d -= 1;
                    current.push(tok.to_string());
                }
                "," if d == 0 => {
                    if !current.is_empty() {
                        args.push(parse_type_tokens(&current));
                        current.clear();
                    }
                }
                _ => current.push(tok.to_string()),
            }
        }
        if !current.is_empty() {
            args.push(parse_type_tokens(&current));
        }

        return match head {
            "List" => Type::List(Box::new(args.into_iter().next().unwrap_or(Type::Unknown))),
            "Sequence" => {
                Type::Sequence(Box::new(args.into_iter().next().unwrap_or(Type::Unknown)))
            }
            "Set" => Type::Set(Box::new(args.into_iter().next().unwrap_or(Type::Unknown))),
            "Option" => Type::Option(Box::new(args.into_iter().next().unwrap_or(Type::Unknown))),
            "Map" => {
                let mut it = args.into_iter();
                let k = it.next().unwrap_or(Type::Unknown);
                let v = it.next().unwrap_or(Type::Unknown);
                Type::Map(Box::new(k), Box::new(v))
            }
            "Result" => {
                let mut it = args.into_iter();
                let ok = it.next().unwrap_or(Type::Unknown);
                let err = it.next().unwrap_or(Type::Unknown);
                Type::Result(Box::new(ok), Box::new(err))
            }
            "Vec" => Type::List(Box::new(args.into_iter().next().unwrap_or(Type::Unknown))),
            _ => Type::Named(head.to_string()),
        };
    }

    // Tuple type: ( A, B, C )
    if head == "(" && clean.last() == Some(&")") {
        let inner = &clean[1..clean.len() - 1];
        if inner.is_empty() {
            return Type::Unit;
        }
        // Split on commas at depth 0
        let mut elems: Vec<Type> = Vec::new();
        let mut current: Vec<String> = Vec::new();
        let mut d = 0i32;
        for tok in inner {
            match *tok {
                "(" | "<" => {
                    d += 1;
                    current.push(tok.to_string());
                }
                ")" | ">" => {
                    d -= 1;
                    current.push(tok.to_string());
                }
                "," if d == 0 => {
                    if !current.is_empty() {
                        elems.push(parse_type_tokens(&current));
                        current.clear();
                    }
                }
                _ => current.push(tok.to_string()),
            }
        }
        if !current.is_empty() {
            elems.push(parse_type_tokens(&current));
        }
        return Type::Tuple(elems);
    }

    // Fallback: treat as named type
    if let Some(ty) = builtin_type(head) {
        return ty;
    }
    Type::Named(head.to_string())
}

// ---------------------------------------------------------------------------
// Type environment construction
// ---------------------------------------------------------------------------

/// Build a `TypeEnv` from a resolved symbol table and the source AST.
///
/// First walks the symbol table for top-level declarations, then walks the
/// AST to extract actual parameter types from `Param.ty` token sequences
/// and function return types from `FnDef.return_ty`.
fn build_type_env(symbols: &SymbolTable, source: &assura_parser::ast::SourceFile) -> TypeEnv {
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
fn build_type_env_from_hir(hir: &assura_hir::HirFile) -> TypeEnv {
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

// ---------------------------------------------------------------------------
// Type display (for error messages)
// ---------------------------------------------------------------------------

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Int => write!(f, "Int"),
            Type::Nat => write!(f, "Nat"),
            Type::Float => write!(f, "Float"),
            Type::Bool => write!(f, "Bool"),
            Type::String => write!(f, "String"),
            Type::Bytes => write!(f, "Bytes"),
            Type::Unit => write!(f, "Unit"),
            Type::Never => write!(f, "Never"),
            Type::U8 => write!(f, "U8"),
            Type::U16 => write!(f, "U16"),
            Type::U32 => write!(f, "U32"),
            Type::U64 => write!(f, "U64"),
            Type::I8 => write!(f, "I8"),
            Type::I16 => write!(f, "I16"),
            Type::I32 => write!(f, "I32"),
            Type::I64 => write!(f, "I64"),
            Type::F32 => write!(f, "F32"),
            Type::F64 => write!(f, "F64"),
            Type::List(t) => write!(f, "List<{t}>"),
            Type::Map(k, v) => write!(f, "Map<{k}, {v}>"),
            Type::Set(t) => write!(f, "Set<{t}>"),
            Type::Option(t) => write!(f, "Option<{t}>"),
            Type::Result(t, e) => write!(f, "Result<{t}, {e}>"),
            Type::Sequence(t) => write!(f, "Sequence<{t}>"),
            Type::Named(n) => write!(f, "{n}"),
            Type::TypeParam(n) => write!(f, "{n}"),
            Type::Fn { params, ret } => {
                write!(f, "fn(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{p}")?;
                }
                write!(f, ") -> {ret}")
            }
            Type::Tuple(elems) => {
                write!(f, "(")?;
                for (i, t) in elems.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{t}")?;
                }
                write!(f, ")")
            }
            Type::Refined { base, predicate } => {
                if predicate.is_empty() {
                    write!(f, "{base}")
                } else {
                    write!(f, "{{ x : {base} | {predicate} }}")
                }
            }
            Type::Unknown => write!(f, "Unknown"),
            Type::Error => write!(f, "<error>"),
        }
    }
}

// ---------------------------------------------------------------------------
// Generic type instantiation (T015)
// ---------------------------------------------------------------------------

/// Expected number of type arguments for built-in generic types.
fn builtin_generic_arity(name: &str) -> Option<usize> {
    match name {
        "List" | "Set" | "Option" | "Sequence" => Some(1),
        "Map" | "Result" => Some(2),
        _ => None,
    }
}

/// Check that a generic type instantiation has the correct number of type
/// arguments.
///
/// For built-in generic types (`List`, `Map`, `Set`, `Option`, `Result`,
/// `Sequence`), the expected arity is hardcoded. For user-defined generic
/// types, the expected arity is taken from the `type_params` count in the
/// symbol table (looked up from the source AST declarations).
///
/// Returns `Ok(())` on success, or `Err(TypeError)` with code A03002 if the
/// argument count does not match.
pub(crate) fn check_generic_instantiation(
    type_name: &str,
    type_args: &[Type],
    span: &Range<usize>,
    source: &assura_parser::ast::SourceFile,
) -> Result<(), TypeError> {
    // Try built-in generic arity first
    if let Some(expected) = builtin_generic_arity(type_name) {
        let actual = type_args.len();
        if actual != expected {
            return Err(TypeError {
                code: "A03002".into(),
                message: format!(
                    "wrong number of type arguments for `{type_name}`: \
                     expected {expected}, found {actual}"
                ),
                span: span.clone(),
                secondary: None,
            });
        }
        return Ok(());
    }

    // Look up user-defined type parameter count from source AST
    if let Some(expected) = user_defined_type_param_count(type_name, source) {
        let actual = type_args.len();
        if actual != expected {
            return Err(TypeError {
                code: "A03002".into(),
                message: format!(
                    "wrong number of type arguments for `{type_name}`: \
                     expected {expected}, found {actual}"
                ),
                span: span.clone(),
                secondary: None,
            });
        }
        return Ok(());
    }

    // Unknown type name; not our problem here (name resolution handles it)
    Ok(())
}

/// Look up the number of type parameters for a user-defined type, contract,
/// or enum by scanning the source AST declarations.
fn user_defined_type_param_count(
    name: &str,
    source: &assura_parser::ast::SourceFile,
) -> Option<usize> {
    for decl in &source.decls {
        match &decl.node {
            Decl::TypeDef(t) if t.name == name => return Some(t.type_params.len()),
            Decl::EnumDef(e) if e.name == name => return Some(e.type_params.len()),
            Decl::Contract(c) if c.name == name => return Some(c.type_params.len()),
            _ => {}
        }
    }
    None
}

/// Substitute type parameters with concrete types in a `Type`.
///
/// Given a mapping from type parameter names to concrete types, recursively
/// replaces every `Type::TypeParam(name)` that appears in `bindings` with
/// the corresponding concrete type. Types not in the bindings map are left
/// unchanged.
#[cfg(test)]
pub(crate) fn substitute(ty: &Type, bindings: &HashMap<std::string::String, Type>) -> Type {
    match ty {
        Type::TypeParam(name) => bindings.get(name).cloned().unwrap_or_else(|| ty.clone()),
        Type::List(inner) => Type::List(Box::new(substitute(inner, bindings))),
        Type::Set(inner) => Type::Set(Box::new(substitute(inner, bindings))),
        Type::Option(inner) => Type::Option(Box::new(substitute(inner, bindings))),
        Type::Sequence(inner) => Type::Sequence(Box::new(substitute(inner, bindings))),
        Type::Map(k, v) => Type::Map(
            Box::new(substitute(k, bindings)),
            Box::new(substitute(v, bindings)),
        ),
        Type::Result(t, e) => Type::Result(
            Box::new(substitute(t, bindings)),
            Box::new(substitute(e, bindings)),
        ),
        Type::Fn { params, ret } => Type::Fn {
            params: params.iter().map(|p| substitute(p, bindings)).collect(),
            ret: Box::new(substitute(ret, bindings)),
        },
        Type::Refined { base, predicate } => Type::Refined {
            base: Box::new(substitute(base, bindings)),
            predicate: predicate.clone(),
        },
        // All other types are leaves; no substitution needed
        _ => ty.clone(),
    }
}

/// Instantiate a built-in generic type with concrete type arguments.
///
/// Given a built-in generic name and validated type arguments, returns the
/// fully instantiated `Type`. Panics if the argument count is wrong (caller
/// should validate via `check_generic_instantiation` first).
#[cfg(test)]
pub(crate) fn instantiate_builtin_generic(name: &str, args: Vec<Type>) -> Option<Type> {
    match name {
        "List" => Some(Type::List(Box::new(args.into_iter().next()?))),
        "Set" => Some(Type::Set(Box::new(args.into_iter().next()?))),
        "Option" => Some(Type::Option(Box::new(args.into_iter().next()?))),
        "Sequence" => Some(Type::Sequence(Box::new(args.into_iter().next()?))),
        "Map" => {
            let mut it = args.into_iter();
            let k = it.next()?;
            let v = it.next()?;
            Some(Type::Map(Box::new(k), Box::new(v)))
        }
        "Result" => {
            let mut it = args.into_iter();
            let t = it.next()?;
            let e = it.next()?;
            Some(Type::Result(Box::new(t), Box::new(e)))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// T015: Generic instantiation arity checking (run on all type exprs)
// ---------------------------------------------------------------------------

/// Walk all declarations and check that generic type instantiations
/// (e.g. `List<Int, Bool>`) have the correct number of type arguments.
fn run_generic_instantiation_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    use assura_parser::ast::TypeExpr;
    let mut errors = Vec::new();

    fn check_type_expr(
        te: &TypeExpr,
        span: &Range<usize>,
        source: &assura_parser::ast::SourceFile,
        errors: &mut Vec<TypeError>,
    ) {
        if let TypeExpr::Generic(name, args) = te {
            // Check arity
            let type_args: Vec<Type> = args.iter().map(type_from_expr).collect();
            if let Err(e) = check_generic_instantiation(name, &type_args, span, source) {
                errors.push(e);
            }
            // Recurse into type arguments
            for arg in args {
                check_type_expr(arg, span, source, errors);
            }
        }
        if let TypeExpr::Fn { params, ret } = te {
            for p in params {
                check_type_expr(p, span, source, errors);
            }
            check_type_expr(ret, span, source, errors);
        }
        if let TypeExpr::Refined { base, .. } = te {
            check_type_expr(base, span, source, errors);
        }
    }

    fn check_params(
        params: &[assura_parser::ast::Param],
        span: &Range<usize>,
        source: &assura_parser::ast::SourceFile,
        errors: &mut Vec<TypeError>,
    ) {
        for p in params {
            if let Some(te) = &p.parsed_type {
                check_type_expr(te, span, source, errors);
            }
        }
    }

    fn check_fields(
        fields: &[assura_parser::ast::FieldDef],
        span: &Range<usize>,
        source: &assura_parser::ast::SourceFile,
        errors: &mut Vec<TypeError>,
    ) {
        for f in fields {
            if let Some(te) = &f.parsed_type {
                check_type_expr(te, span, source, errors);
            }
        }
    }

    for decl in &source.decls {
        let span = &decl.span;
        match &decl.node {
            Decl::Contract(c) => {
                for clause in &c.clauses {
                    if let ClauseKind::Input | ClauseKind::Output = &clause.kind {
                        // Params may be in clause bodies; handled by param extraction
                    }
                }
            }
            Decl::TypeDef(td) => {
                if let assura_parser::ast::TypeBody::Struct(fields) = &td.body {
                    check_fields(fields, span, source, &mut errors);
                }
            }
            Decl::FnDef(f) => {
                check_params(&f.params, span, source, &mut errors);
                if let Some(te) = &f.return_type_expr {
                    check_type_expr(te, span, source, &mut errors);
                }
            }
            Decl::Extern(e) => {
                check_params(&e.params, span, source, &mut errors);
                if let Some(te) = &e.return_type_expr {
                    check_type_expr(te, span, source, &mut errors);
                }
            }
            _ => {}
        }
    }

    errors
}

// ---------------------------------------------------------------------------
// Ghost function effect checking (T043 CORE.1)
// ---------------------------------------------------------------------------

/// Extract the declared effect set from an `effects` clause on a function.
///
/// If no `effects` clause exists, returns `None` (meaning no explicit
/// declaration, which is NOT the same as pure).
fn extract_fn_effects(f: &assura_parser::ast::FnDef) -> Option<Vec<std::string::String>> {
    for clause in &f.clauses {
        if clause.kind == ClauseKind::Effects {
            // Extract effect names from the clause body
            let mut names = Vec::new();
            extract_effect_names(&clause.body, &mut names);
            return Some(names);
        }
    }
    None
}

/// Recursively extract effect name strings from an expression.
fn extract_effect_names(expr: &Expr, names: &mut Vec<std::string::String>) {
    match expr {
        Expr::Ident(s) => names.push(s.clone()),
        Expr::Raw(tokens) => {
            for tok in tokens {
                let trimmed = tok.trim().to_string();
                if !trimmed.is_empty() && trimmed != "," {
                    names.push(trimmed);
                }
            }
        }
        Expr::Block(items) => {
            for item in items {
                extract_effect_names(item, names);
            }
        }
        _ => {}
    }
}

/// Check that a lemma function has pure effects.
///
/// Lemma functions are proof functions that generate no runtime code.
/// They cannot perform side effects. If an `effects` clause is present
/// and declares non-pure effects, emit A55001.
pub(crate) fn check_lemma_fn_effects(
    f: &assura_parser::ast::FnDef,
    span: &Range<usize>,
    errors: &mut Vec<TypeError>,
) {
    if let Some(effects) = extract_fn_effects(f) {
        let has_non_pure = effects.iter().any(|e| e != "pure");
        if has_non_pure {
            let effect_list = effects
                .iter()
                .filter(|e| *e != "pure")
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            errors.push(TypeError {
                code: "A55001".into(),
                message: format!(
                    "lemma function `{}` has non-pure effects: {effect_list}; \
                     lemma functions must be pure (no side effects)",
                    f.name,
                ),
                span: span.clone(),
                secondary: None,
            });
        }
    }
    // If no effects clause is present, lemma fns are implicitly pure: OK.
}

/// Check that a ghost function has pure effects.
///
/// Ghost functions exist only for verification; they cannot perform side
/// effects. If an `effects` clause is present and declares non-pure effects,
/// emit A54001.
pub(crate) fn check_ghost_fn_effects(
    f: &assura_parser::ast::FnDef,
    span: &Range<usize>,
    errors: &mut Vec<TypeError>,
) {
    if let Some(effects) = extract_fn_effects(f) {
        // "pure" or an empty list means no effects: that's fine for ghost fns.
        let has_non_pure = effects.iter().any(|e| e != "pure");
        if has_non_pure {
            let effect_list = effects
                .iter()
                .filter(|e| *e != "pure")
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            errors.push(TypeError {
                code: "A54001".into(),
                message: format!(
                    "ghost function `{}` has non-pure effects: {effect_list}; \
                     ghost functions must be pure (no side effects)",
                    f.name,
                ),
                span: span.clone(),
                secondary: None,
            });
        }
    }
    // If no effects clause is present, ghost fns are implicitly pure: OK.
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Type-check a resolved file.
///
/// Builds a type environment from the symbol table. For T013 this always
/// succeeds (no expression-level checking yet). Returns a `TypedFile`
/// containing the resolved file and its type environment, or a list of
/// `TypeError`s.
pub fn type_check(resolved: &ResolvedFile) -> Result<TypedFile, Vec<TypeError>> {
    type_check_with_config(resolved, &assura_config::TypeCheckConfig::default())
}

/// Type-check a resolved file with cross-module type information.
///
/// Unlike [`type_check_with_config`], this populates the `TypeEnv` with
/// type information from imported modules so that cross-file references
/// (contract input/output types, struct fields, enum variants) resolve
/// to concrete types instead of `Type::Unknown`.
pub fn type_check_with_modules(
    resolved: &ResolvedFile,
    modules: &HashMap<String, ResolvedFile>,
    config: &assura_config::TypeCheckConfig,
) -> Result<TypedFile, Vec<TypeError>> {
    let mut type_env = build_type_env(&resolved.symbols, &resolved.source);

    // Inject type information from imported modules
    for imp in &resolved.imports {
        if imp.status != ImportStatus::Resolved {
            continue;
        }
        let module_key = imp.path.join(".");
        if let Some(imported_resolved) = modules.get(&module_key) {
            inject_imported_types(&mut type_env, imp, &imported_resolved.source);
        }
    }

    // Run the same checking pipeline as type_check_with_config
    let source = &resolved.source;
    let mut errors = check_clause_bodies(source, &type_env);
    errors.extend(run_axiomatic_checks(source, &resolved.symbols));
    errors.extend(run_liveness_checks(source));
    errors.extend(run_crud_auth_checks(source));
    errors.extend(run_linearity_checks(source));
    errors.extend(run_typestate_checks(source));

    let mut effect_errors = run_effect_checks(source);
    if !config.allowed_effects.is_empty() || !config.denied_effects.is_empty() {
        effect_errors.retain(|e| {
            let msg = &e.message;
            !config.allowed_effects.iter().any(|a| msg.contains(a))
        });
    }
    if config.strict_effects {
        errors.extend(effect_errors);
    } else {
        errors.extend(effect_errors.into_iter().filter(|e| e.code != "A07003"));
    }

    errors.extend(run_taint_checks(source));
    errors.extend(run_info_flow_checks(source));
    errors.extend(run_ffi_checks(source));
    errors.extend(run_error_propagation_checks(source));
    errors.extend(run_frame_checks(source, &type_env, &resolved.symbols));
    let (totality_errors, pending_decrease_checks) = run_totality_checks(source);
    errors.extend(totality_errors);
    errors.extend(run_fixed_width_checks(source, &type_env));
    errors.extend(run_collection_contract_checks(source));
    errors.extend(run_match_exhaustiveness_checks(source, &resolved.symbols));
    errors.extend(run_constant_time_checks(source));
    errors.extend(run_determinism_checks(source));
    errors.extend(run_memory_checks(source));
    errors.extend(run_secure_erasure_checks(source));
    errors.extend(run_interface_checks(source));
    errors.extend(run_structural_invariant_checks(source));
    errors.extend(run_shared_mem_checks(source));
    errors.extend(run_lock_order_checks(source));
    errors.extend(run_weak_memory_checks(source));
    errors.extend(run_allocator_checks(source));
    errors.extend(run_circular_buffer_checks(source));
    errors.extend(run_callback_reentrancy_checks(source));
    errors.extend(run_temporal_deadline_checks(source));
    errors.extend(run_binary_format_checks(source));
    errors.extend(run_bit_level_checks(source));
    errors.extend(run_string_encoding_checks(source));
    errors.extend(run_checksum_checks(source));
    errors.extend(run_protocol_grammar_checks(source));
    errors.extend(run_opaque_function_checks(source));
    errors.extend(run_crash_recovery_checks(source));
    errors.extend(run_page_cache_checks(source));
    errors.extend(run_mvcc_checks(source));
    errors.extend(run_rollback_checks(source));
    errors.extend(run_monotonic_state_checks(source));
    errors.extend(run_storage_failure_checks(source));
    errors.extend(run_numerical_precision_checks(source));
    errors.extend(run_precomputed_table_checks(source));
    errors.extend(run_platform_abstraction_checks(source));
    errors.extend(run_feature_flag_checks(source));
    errors.extend(run_resource_limit_checks(source));
    errors.extend(run_unsafe_escape_checks(source));
    errors.extend(run_complexity_bound_checks(source));
    errors.extend(run_behavioral_equivalence_checks(source));
    errors.extend(run_multi_pass_refinement_checks(source));
    errors.extend(run_incremental_contract_checks(source));
    errors.extend(run_scoped_invariant_checks(source));
    errors.extend(run_contract_composition_checks(source));
    errors.extend(run_contract_library_checks(source));
    errors.extend(run_crypto_conformance_checks(source));
    errors.extend(run_codec_registry_checks(source));
    errors.extend(run_generic_instantiation_checks(source));

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(TypedFile {
        resolved: Arc::new(resolved.clone()),
        pending_decrease_checks,
        type_env,
        hir: None,
    })
}

/// Inject type information from an imported module's AST into the type
/// environment. Adds concrete types for imported contracts, services,
/// type definitions, enum variants, and function signatures.
fn inject_imported_types(
    env: &mut TypeEnv,
    imp: &assura_resolve::ResolvedImport,
    source: &assura_parser::ast::SourceFile,
) {
    // Collect the names this import brings into scope
    let imported_names: Vec<&str> = if !imp.items.is_empty() {
        // Selective import: `import math { Add, Vector }`
        imp.items.iter().map(|s| s.as_str()).collect()
    } else if let Some(alias) = &imp.alias {
        // Aliased import: `import math as m`
        vec![alias.as_str()]
    } else if let Some(last) = imp.path.last() {
        // Default import: `import math` brings the module name
        vec![last.as_str()]
    } else {
        return;
    };

    for decl in &source.decls {
        match &decl.node {
            Decl::Contract(c) if imported_names.contains(&c.name.as_str()) => {
                // Register the contract name as a Named type
                env.insert(c.name.clone(), Type::Named(c.name.clone()));
                // Register input params so callers know the contract's signature
                for clause in &c.clauses {
                    if clause.kind == ClauseKind::Input {
                        register_input_clause_params(&clause.body, env);
                    }
                }
            }
            Decl::Service(s) if imported_names.contains(&s.name.as_str()) => {
                env.insert(s.name.clone(), Type::Named(s.name.clone()));
                // Register operation/query signatures
                for item in &s.items {
                    let (name, clauses) = match item {
                        ServiceItem::Operation { name, clauses } => (name, clauses),
                        ServiceItem::Query { name, clauses } => (name, clauses),
                        _ => continue,
                    };
                    let mut param_types = Vec::new();
                    for clause in clauses {
                        if clause.kind == ClauseKind::Input {
                            collect_input_param_types(&clause.body, &mut param_types);
                        }
                    }
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
            Decl::TypeDef(td) if imported_names.contains(&td.name.as_str()) => {
                env.insert(td.name.clone(), Type::Named(td.name.clone()));
                if let assura_parser::ast::TypeBody::Struct(fields) = &td.body {
                    let field_types: Vec<(String, Type)> = fields
                        .iter()
                        .map(|f| (f.name.clone(), parse_type_tokens(&f.ty)))
                        .collect();
                    env.struct_fields.insert(td.name.clone(), field_types);
                }
            }
            Decl::EnumDef(e) if imported_names.contains(&e.name.as_str()) => {
                env.insert(e.name.clone(), Type::Named(e.name.clone()));
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
            Decl::FnDef(f) if imported_names.contains(&f.name.as_str()) => {
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
            Decl::Extern(e) if imported_names.contains(&e.name.as_str()) => {
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
            _ => {}
        }
    }
}

/// Type-check from an HIR file. This is the preferred entry point when the
/// HIR lowering pass has already been run.
pub fn type_check_hir(hir: &assura_hir::HirFile) -> Result<TypedFile, Vec<TypeError>> {
    type_check_hir_with_config(hir, &assura_config::TypeCheckConfig::default())
}

/// Type-check from an HIR file using the given configuration.
///
/// Uses `build_type_env_from_hir` to construct the type environment from
/// structured HIR types instead of raw token parsing.
pub fn type_check_hir_with_config(
    hir: &assura_hir::HirFile,
    config: &assura_config::TypeCheckConfig,
) -> Result<TypedFile, Vec<TypeError>> {
    let resolved = hir.resolved();
    let type_env = build_type_env_from_hir(hir);

    // Check clause bodies using HIR declarations (structured types for
    // return types, HirExpr->Expr bridge for inference)
    let source = &resolved.source;
    let mut errors = check_clause_bodies_hir(hir, &type_env);
    errors.extend(run_axiomatic_checks(source, &resolved.symbols));
    errors.extend(run_liveness_checks(source));
    errors.extend(run_crud_auth_checks(source));
    errors.extend(run_linearity_checks(source));
    errors.extend(run_typestate_checks(source));

    // Apply config-driven effect checking (same logic as AST path)
    let mut effect_errors = run_effect_checks(source);
    if !config.allowed_effects.is_empty() || !config.denied_effects.is_empty() {
        effect_errors.retain(|e| !config.allowed_effects.iter().any(|a| e.message.contains(a)));
    }
    if config.strict_effects {
        errors.extend(effect_errors);
    } else {
        errors.extend(effect_errors.into_iter().filter(|e| e.code != "A07003"));
    }
    errors.extend(run_taint_checks(source));
    errors.extend(run_info_flow_checks(source));
    errors.extend(run_ffi_checks(source));
    errors.extend(run_error_propagation_checks(source));
    errors.extend(run_frame_checks(source, &type_env, &resolved.symbols));
    let (totality_errors, pending_decrease_checks) = run_totality_checks(source);
    errors.extend(totality_errors);
    errors.extend(run_fixed_width_checks(source, &type_env));
    errors.extend(run_collection_contract_checks(source));
    errors.extend(run_match_exhaustiveness_checks(source, &resolved.symbols));
    errors.extend(run_constant_time_checks(source));
    errors.extend(run_determinism_checks(source));
    errors.extend(run_memory_checks(source));
    errors.extend(run_secure_erasure_checks(source));
    errors.extend(run_interface_checks(source));
    errors.extend(run_structural_invariant_checks(source));
    errors.extend(run_shared_mem_checks(source));
    errors.extend(run_lock_order_checks(source));
    errors.extend(run_weak_memory_checks(source));
    errors.extend(run_allocator_checks(source));
    errors.extend(run_circular_buffer_checks(source));
    errors.extend(run_callback_reentrancy_checks(source));
    errors.extend(run_temporal_deadline_checks(source));
    errors.extend(run_binary_format_checks(source));
    errors.extend(run_bit_level_checks(source));
    errors.extend(run_string_encoding_checks(source));
    errors.extend(run_checksum_checks(source));
    errors.extend(run_protocol_grammar_checks(source));
    errors.extend(run_opaque_function_checks(source));
    errors.extend(run_crash_recovery_checks(source));
    errors.extend(run_page_cache_checks(source));
    errors.extend(run_mvcc_checks(source));
    errors.extend(run_rollback_checks(source));
    errors.extend(run_monotonic_state_checks(source));
    errors.extend(run_storage_failure_checks(source));
    errors.extend(run_numerical_precision_checks(source));
    errors.extend(run_precomputed_table_checks(source));
    errors.extend(run_platform_abstraction_checks(source));
    errors.extend(run_feature_flag_checks(source));
    errors.extend(run_resource_limit_checks(source));
    errors.extend(run_unsafe_escape_checks(source));
    errors.extend(run_complexity_bound_checks(source));
    errors.extend(run_behavioral_equivalence_checks(source));
    errors.extend(run_multi_pass_refinement_checks(source));
    errors.extend(run_incremental_contract_checks(source));
    errors.extend(run_scoped_invariant_checks(source));
    errors.extend(run_contract_composition_checks(source));
    errors.extend(run_contract_library_checks(source));
    errors.extend(run_crypto_conformance_checks(source));
    errors.extend(run_codec_registry_checks(source));

    // T015: generic instantiation arity checking
    errors.extend(run_generic_instantiation_checks(source));

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(TypedFile {
        resolved: Arc::clone(&hir.resolved),
        pending_decrease_checks,
        type_env,
        hir: Some(hir.clone()),
    })
}

/// Type-check a resolved file using the given configuration.
///
/// `config.strict_effects` controls whether the effect checker runs.
/// `config.warn_unused_imports` is reserved for future import analysis.
pub fn type_check_with_config(
    resolved: &ResolvedFile,
    config: &assura_config::TypeCheckConfig,
) -> Result<TypedFile, Vec<TypeError>> {
    let type_env = build_type_env(&resolved.symbols, &resolved.source);

    // T014: walk clause bodies and infer expression types. Collect any
    // concrete type-mismatch errors (A03001). Unknown types from unresolved
    // identifiers are silently propagated (no false positives).
    let mut errors = check_clause_bodies(&resolved.source, &type_env);

    // T077: check axiomatic definition references and usage
    errors.extend(run_axiomatic_checks(&resolved.source, &resolved.symbols));
    // G006: validate liveness block structure
    errors.extend(run_liveness_checks(&resolved.source));

    // T109: check CRUD/auth coverage on services
    errors.extend(run_crud_auth_checks(&resolved.source));

    // T031/T032: linearity checking (usage tracking + context splitting)
    errors.extend(run_linearity_checks(&resolved.source));

    // T034: typestate checking (DFA state transitions on services)
    errors.extend(run_typestate_checks(&resolved.source));

    // T036: effect checking (declared vs actual effect containment)
    // Apply config: allowed/denied effects and strict mode
    let mut effect_errors = run_effect_checks(&resolved.source);
    if !config.allowed_effects.is_empty() || !config.denied_effects.is_empty() {
        effect_errors.retain(|e| {
            // Keep errors for denied effects; filter out errors for allowed effects
            let msg = &e.message;
            !config.allowed_effects.iter().any(|a| msg.contains(a))
        });
    }
    if config.strict_effects {
        errors.extend(effect_errors);
    } else {
        // In non-strict mode, only keep A07001 (undeclared effect usage),
        // not A07003 (unknown effect names)
        errors.extend(effect_errors.into_iter().filter(|e| e.code != "A07003"));
    }

    // T047: taint tracking (untrusted data flow analysis)
    errors.extend(run_taint_checks(&resolved.source));

    // S003: information flow tracking (security label propagation)
    errors.extend(run_info_flow_checks(&resolved.source));

    // T058: FFI boundary contracts (extern declarations)
    errors.extend(run_ffi_checks(&resolved.source));

    // T064: error propagation checking (must_propagate on error types)
    errors.extend(run_error_propagation_checks(&resolved.source));

    // T045: frame checking (modifies clause scope validation)
    errors.extend(run_frame_checks(
        &resolved.source,
        &type_env,
        &resolved.symbols,
    ));

    // T053: totality checking (termination via decreases measures)
    let (totality_errors, pending_decrease_checks) = run_totality_checks(&resolved.source);
    errors.extend(totality_errors);

    // T055: fixed-width integer overflow detection
    errors.extend(run_fixed_width_checks(&resolved.source, &type_env));

    // T108: collection contracts validation (sort, filter, map, reverse, deduplicate)
    errors.extend(run_collection_contract_checks(&resolved.source));

    // T017: match expression exhaustiveness checking
    errors.extend(run_match_exhaustiveness_checks(
        &resolved.source,
        &resolved.symbols,
    ));

    // T059: constant-time checking (secret-dependent branching/indexing)
    errors.extend(run_constant_time_checks(&resolved.source));

    // T067: determinism checking (pure functions must be deterministic)
    errors.extend(run_determinism_checks(&resolved.source));

    // T046: memory safety checking (buffer bounds via annotations)
    errors.extend(run_memory_checks(&resolved.source));

    // T060: secure erasure checking (sensitive data must be zeroed)
    errors.extend(run_secure_erasure_checks(&resolved.source));

    // T062: interface contracts (method completeness, signature matching)
    errors.extend(run_interface_checks(&resolved.source));

    // T063: structural invariants (recursive type properties)
    errors.extend(run_structural_invariant_checks(&resolved.source));

    // T065: shared memory protocols (concurrent access validation)
    errors.extend(run_shared_mem_checks(&resolved.source));

    // T068: lock ordering (deadlock prevention via static hierarchy)
    errors.extend(run_lock_order_checks(&resolved.source));

    // G007: weak memory ordering validation (CONC.6)
    errors.extend(run_weak_memory_checks(&resolved.source));

    // Domain checkers from domain.rs
    errors.extend(run_allocator_checks(&resolved.source));
    errors.extend(run_circular_buffer_checks(&resolved.source));
    errors.extend(run_callback_reentrancy_checks(&resolved.source));
    errors.extend(run_temporal_deadline_checks(&resolved.source));
    errors.extend(run_binary_format_checks(&resolved.source));
    errors.extend(run_bit_level_checks(&resolved.source));
    errors.extend(run_string_encoding_checks(&resolved.source));
    errors.extend(run_checksum_checks(&resolved.source));
    errors.extend(run_protocol_grammar_checks(&resolved.source));
    errors.extend(run_opaque_function_checks(&resolved.source));
    errors.extend(run_crash_recovery_checks(&resolved.source));
    errors.extend(run_page_cache_checks(&resolved.source));
    errors.extend(run_mvcc_checks(&resolved.source));
    errors.extend(run_rollback_checks(&resolved.source));
    errors.extend(run_monotonic_state_checks(&resolved.source));
    errors.extend(run_storage_failure_checks(&resolved.source));
    errors.extend(run_numerical_precision_checks(&resolved.source));
    errors.extend(run_precomputed_table_checks(&resolved.source));
    errors.extend(run_platform_abstraction_checks(&resolved.source));
    errors.extend(run_feature_flag_checks(&resolved.source));
    errors.extend(run_resource_limit_checks(&resolved.source));
    errors.extend(run_unsafe_escape_checks(&resolved.source));
    errors.extend(run_complexity_bound_checks(&resolved.source));
    errors.extend(run_behavioral_equivalence_checks(&resolved.source));
    errors.extend(run_multi_pass_refinement_checks(&resolved.source));
    errors.extend(run_incremental_contract_checks(&resolved.source));
    errors.extend(run_scoped_invariant_checks(&resolved.source));
    errors.extend(run_contract_composition_checks(&resolved.source));
    errors.extend(run_contract_library_checks(&resolved.source));
    errors.extend(run_crypto_conformance_checks(&resolved.source));
    errors.extend(run_codec_registry_checks(&resolved.source));

    // T015: generic instantiation arity checking
    errors.extend(run_generic_instantiation_checks(&resolved.source));

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(TypedFile {
        resolved: Arc::new(resolved.clone()),
        pending_decrease_checks,
        type_env,
        hir: None,
    })
}

/// G006/T094: Validate liveness blocks have required structure.
///
/// Checks that liveness blocks contain at least one `prove` clause
/// and that `leads_to` obligations have accompanying `assume fair`.
fn run_liveness_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut errors = Vec::new();
    for decl in &source.decls {
        if let Decl::Block {
            kind, name, body, ..
        } = &decl.node
        {
            if *kind != BlockKind::Liveness {
                continue;
            }
            let has_prove = body
                .iter()
                .any(|c| matches!(&c.kind, ClauseKind::Other(k) if k == "prove"));
            if !has_prove {
                errors.push(TypeError {
                    code: "A-CORE-030".into(),
                    message: format!(
                        "liveness block `{name}` has no `prove` clause; \
                         at least one liveness property must be stated"
                    ),
                    span: decl.span.clone(),
                    secondary: None,
                });
            }
            let has_leads_to = body.iter().any(|c| {
                matches!(&c.kind, ClauseKind::Other(k) if k == "prove")
                    && expr_contains_text(&c.body, "leads_to")
            });
            let has_fair = body.iter().any(|c| {
                matches!(&c.kind, ClauseKind::Other(k) if k == "assume")
                    && expr_contains_text(&c.body, "fair")
            });
            if has_leads_to && !has_fair {
                errors.push(TypeError {
                    code: "A-CORE-031".into(),
                    message: format!(
                        "liveness block `{name}` uses `leads_to` but has no \
                         `assume fair` clause; fairness is required for \
                         leads-to proofs"
                    ),
                    span: decl.span.clone(),
                    secondary: None,
                });
            }
        }
    }
    errors
}

/// Helper: check if an expression tree contains a text reference.
fn expr_contains_text(expr: &Expr, text: &str) -> bool {
    match expr {
        Expr::Ident(s) => s == text,
        Expr::Raw(tokens) => tokens.iter().any(|t| t == text),
        Expr::Block(exprs) | Expr::List(exprs) => exprs.iter().any(|e| expr_contains_text(e, text)),
        Expr::Call { func, args } => {
            expr_contains_text(func, text) || args.iter().any(|a| expr_contains_text(a, text))
        }
        _ => false,
    }
}

/// T077: Scan for axiomatic blocks and validate references/usage.
fn run_axiomatic_checks(
    source: &assura_parser::ast::SourceFile,
    symbols: &assura_resolve::SymbolTable,
) -> Vec<TypeError> {
    let mut checker = AxiomaticDefChecker::new();
    // First pass: collect all axiom names
    let axiom_names: Vec<String> = source
        .decls
        .iter()
        .filter_map(|d| {
            if let Decl::Block { kind, name, .. } = &d.node
                && *kind == BlockKind::Axiomatic
            {
                Some(name.clone())
            } else {
                None
            }
        })
        .collect();
    // Second pass: declare axioms with references extracted from their bodies
    for decl in &source.decls {
        if let Decl::Block {
            kind, name, body, ..
        } = &decl.node
            && *kind == BlockKind::Axiomatic
        {
            let mut refs = Vec::new();
            for clause in body {
                let idents = collect_ident_references(&clause.body);
                for ident in &idents {
                    if axiom_names.contains(ident) && ident != name {
                        refs.push(ident.clone());
                    }
                }
            }
            refs.sort();
            refs.dedup();
            checker.declare_axiom(AxiomDef {
                name: name.clone(),
                span: decl.span.clone(),
                references: refs,
            });
        }
    }
    // Mark axioms as used if they are referenced in clause bodies
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                let refs = collect_ident_references(&clause.body);
                for name in &refs {
                    checker.mark_used(name);
                }
            }
        }
    }
    let known: Vec<&str> = symbols.symbols.iter().map(|s| s.name.as_str()).collect();
    let mut errors = checker.check_references(&known);
    errors.extend(checker.check_unused());
    errors.extend(checker.check_circular());
    errors
}

/// T109: Scan services for CRUD operations and check auth coverage.
fn run_crud_auth_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut errors = Vec::new();
    for decl in &source.decls {
        if let Decl::Service(s) = &decl.node {
            let mut checker = CrudAuthContracts::new();
            for item in &s.items {
                if let ServiceItem::Operation { name, clauses } = item {
                    let has_auth = clauses.iter().any(|c| {
                        matches!(c.kind, ClauseKind::Other(ref k) if k == "auth" || k == "requires_auth")
                    });
                    let crud_type = if name.starts_with("create") || name.starts_with("add") {
                        CrudType::Create
                    } else if name.starts_with("read")
                        || name.starts_with("get")
                        || name.starts_with("list")
                    {
                        CrudType::Read
                    } else if name.starts_with("update") || name.starts_with("set") {
                        CrudType::Update
                    } else if name.starts_with("delete") || name.starts_with("remove") {
                        CrudType::Delete
                    } else {
                        continue;
                    };
                    checker.add_crud(name.clone(), crud_type, has_auth);
                }
            }
            // Add auth policies from service-level auth clauses
            for item in &s.items {
                if let ServiceItem::Operation { name, clauses } = item {
                    for clause in clauses {
                        if let ClauseKind::Other(ref k) = clause.kind
                            && (k == "auth_policy" || k == "role")
                        {
                            let role = extract_ident(&clause.body).unwrap_or("user").to_string();
                            let allow_self = clauses.iter().any(
                                |c| matches!(&c.kind, ClauseKind::Other(k2) if k2 == "allow_self"),
                            );
                            checker.add_auth_policy(name.clone(), role, allow_self);
                        }
                    }
                }
            }
            errors.extend(checker.check_auth_coverage());
            errors.extend(checker.check_delete_protection());
            errors.extend(checker.check_precondition_coverage());
        }
    }
    errors
}

/// T031/T032: Run linearity checks across all declarations.
///
/// For each contract, fn, extern, and service operation, declares input
/// parameters as linear (grade 1) when annotated with `linear` in type
/// tokens, then walks clause bodies counting usages via context splitting.
fn run_linearity_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut errors = Vec::new();
    for decl in &source.decls {
        match &decl.node {
            Decl::Contract(c) => {
                let mut tracker = UsageTracker::new();
                // Declare inputs as linear if they have linear annotation
                for clause in &c.clauses {
                    if clause.kind == ClauseKind::Input {
                        declare_linear_params_from_expr(&clause.body, &mut tracker, &decl.span);
                    }
                }
                // Walk ensures/requires/invariant bodies
                let mut ctx = LinearContext::new(tracker);
                for clause in &c.clauses {
                    if matches!(
                        clause.kind,
                        ClauseKind::Requires | ClauseKind::Ensures | ClauseKind::Invariant
                    ) {
                        errors.extend(check_expr_linearity(&clause.body, &mut ctx));
                    }
                }
                errors.extend(ctx.check());
            }
            Decl::FnDef(f) => {
                let tracker = UsageTracker::new();
                let mut ctx = LinearContext::new(tracker);
                for param in &f.params {
                    let grade = infer_usage_grade(&param.ty);
                    if grade != UsageGrade::Unlimited {
                        ctx.declare(param.name.clone(), grade, decl.span.clone());
                    }
                }
                for clause in &f.clauses {
                    errors.extend(check_expr_linearity(&clause.body, &mut ctx));
                }
                errors.extend(ctx.check());
            }
            Decl::Extern(e) => {
                let tracker = UsageTracker::new();
                let mut ctx = LinearContext::new(tracker);
                for param in &e.params {
                    let grade = infer_usage_grade(&param.ty);
                    if grade != UsageGrade::Unlimited {
                        ctx.declare(param.name.clone(), grade, decl.span.clone());
                    }
                }
                for clause in &e.clauses {
                    errors.extend(check_expr_linearity(&clause.body, &mut ctx));
                }
                errors.extend(ctx.check());
            }
            Decl::Service(s) => {
                for item in &s.items {
                    if let ServiceItem::Operation { clauses, .. }
                    | ServiceItem::Query { clauses, .. } = item
                    {
                        let tracker = UsageTracker::new();
                        let mut ctx = LinearContext::new(tracker);
                        for clause in clauses {
                            errors.extend(check_expr_linearity(&clause.body, &mut ctx));
                        }
                        errors.extend(ctx.check());
                    }
                }
            }
            _ => {}
        }
    }
    errors
}

/// Infer a usage grade from type annotation tokens.
///
/// - `linear` -> Linear (grade 1)
/// - `ghost` or `erased` -> Erased (grade 0)
/// - `exact(N)` -> Exact(N)
/// - otherwise -> Unlimited (grade omega)
fn infer_usage_grade(ty_tokens: &[String]) -> UsageGrade {
    for (i, t) in ty_tokens.iter().enumerate() {
        match t.as_str() {
            "linear" => return UsageGrade::Linear,
            "ghost" | "erased" => return UsageGrade::Erased,
            "exact" => {
                // Look for a number after "exact"
                if let Some(n_str) = ty_tokens.get(i + 1)
                    && let Ok(n) = n_str.parse::<u32>()
                {
                    return UsageGrade::Exact(n);
                }
                return UsageGrade::Linear;
            }
            _ => {}
        }
    }
    UsageGrade::Unlimited
}

/// Helper: declare linear parameters from an input clause expression.
///
/// Handles multiple Expr patterns where `linear` can appear:
/// - `Expr::Raw`: token sequences like `x : linear Int, y : Int`
/// - `Expr::Call`: `input(x as linear Int)` produces Call with Cast args
/// - `Expr::Cast`: single param `x as linear Int`
/// - `Expr::Block`/`Expr::Tuple`: sequences containing linear-annotated items
/// - `Expr::Paren`: unwrap and recurse
fn declare_linear_params_from_expr(
    expr: &Expr,
    tracker: &mut UsageTracker,
    span: &std::ops::Range<usize>,
) {
    match expr {
        Expr::Raw(tokens) => {
            declare_linear_params_from_raw(tokens, tracker, span);
        }
        Expr::Call { args, .. } => {
            for arg in args {
                declare_linear_single_param(arg, tracker, span);
            }
        }
        Expr::Cast { expr: inner, ty } => {
            if ty.contains("linear")
                && let Expr::Ident(name) = inner.as_ref()
            {
                tracker.declare(name.clone(), UsageGrade::Linear, span.clone());
            }
        }
        Expr::Ident(_) => {
            // Single untyped param, no linear annotation possible
        }
        Expr::Paren(inner) => declare_linear_params_from_expr(inner, tracker, span),
        Expr::Tuple(items) | Expr::Block(items) => {
            for item in items {
                declare_linear_single_param(item, tracker, span);
            }
        }
        _ => {}
    }
}

/// Declare a single input parameter as linear if it has a linear annotation.
fn declare_linear_single_param(
    expr: &Expr,
    tracker: &mut UsageTracker,
    span: &std::ops::Range<usize>,
) {
    match expr {
        Expr::Cast { expr: inner, ty } => {
            if ty.contains("linear")
                && let Expr::Ident(name) = inner.as_ref()
            {
                tracker.declare(name.clone(), UsageGrade::Linear, span.clone());
            }
        }
        Expr::Paren(inner) => declare_linear_single_param(inner, tracker, span),
        Expr::Raw(tokens) => {
            declare_linear_params_from_raw(tokens, tracker, span);
        }
        _ => {}
    }
}

/// Parse raw tokens for linear parameter declarations.
fn declare_linear_params_from_raw(
    tokens: &[String],
    tracker: &mut UsageTracker,
    span: &std::ops::Range<usize>,
) {
    let mut i = 0;
    while i < tokens.len() {
        // Look for pattern: name : linear Type  OR  name as linear Type
        let sep = tokens.get(i + 1).map(|s| s.as_str());
        if i + 2 < tokens.len()
            && matches!(sep, Some(":" | "as"))
            && tokens[i + 2..]
                .iter()
                .take_while(|t| *t != ",")
                .any(|t| t == "linear")
        {
            let name = &tokens[i];
            tracker.declare(name.clone(), UsageGrade::Linear, span.clone());
            // Skip to the next parameter (past comma)
            while i < tokens.len() && tokens[i] != "," {
                i += 1;
            }
        }
        i += 1;
    }
}

/// T034: Run typestate checks on services with `states:` declarations.
///
/// For each service with a States item, builds a TypestateChecker with
/// the declared states and validates transitions and operation ordering.
fn run_typestate_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut errors = Vec::new();
    for decl in &source.decls {
        if let Decl::Service(s) = &decl.node {
            // Find states declaration
            let states: Vec<String> = s
                .items
                .iter()
                .filter_map(|item| {
                    if let ServiceItem::States(s) = item {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .flatten()
                .collect();

            if states.is_empty() {
                continue;
            }

            // Build transitions from operation clauses
            let mut transitions = Vec::new();
            for item in &s.items {
                if let ServiceItem::Operation { name, clauses } = item {
                    for clause in clauses {
                        if let ClauseKind::Other(ref k) = clause.kind
                            && (k == "transition" || k == "from_state" || k == "to_state")
                            && let Expr::Raw(tokens) = &clause.body
                            && tokens.len() >= 3
                        {
                            transitions.push((name.clone(), tokens[0].clone(), tokens[2].clone()));
                        }
                    }
                }
            }

            if !transitions.is_empty() {
                let initial = states.first().cloned().unwrap_or_default();
                let mut checker =
                    TypestateChecker::new(states, transitions, initial, decl.span.clone());
                // Validate transitions reference valid states
                for tse in checker.validate_transitions() {
                    errors.push(TypeError {
                        code: tse.code,
                        message: tse.message,
                        span: tse.span,
                        secondary: None,
                    });
                }

                // Validate linearity: typestate variables must be linear
                let has_linear_annotation = s.items.iter().any(|item| {
                    if let ServiceItem::Operation { clauses, .. } = item {
                        clauses
                            .iter()
                            .any(|c| matches!(&c.kind, ClauseKind::Other(k) if k == "linear"))
                    } else {
                        false
                    }
                });
                if let Some(tse) = checker.validate_linear(has_linear_annotation) {
                    errors.push(TypeError {
                        code: tse.code,
                        message: tse.message,
                        span: tse.span,
                        secondary: None,
                    });
                }

                // Simulate transitions in operation order and check consistency
                let mut branch_checkers: Vec<TypestateChecker> = Vec::new();
                for item in &s.items {
                    if let ServiceItem::Operation { name, clauses } = item {
                        let pre_state = checker.current_state().to_string();
                        if let Err(tse) = checker.transition(name, decl.span.clone()) {
                            errors.push(TypeError {
                                code: tse.code,
                                message: tse.message,
                                span: tse.span,
                                secondary: None,
                            });
                        }

                        // Track variable usages in clause bodies
                        let mut usage_tracker = UsageTracker::new();
                        for clause in clauses {
                            expr_usages(&clause.body, &mut usage_tracker);
                        }

                        // Record checker state after each branch for consistency check
                        if !pre_state.is_empty() {
                            branch_checkers.push(TypestateChecker::new(
                                checker.states.clone(),
                                Vec::new(),
                                checker.current_state().to_string(),
                                decl.span.clone(),
                            ));
                        }
                    }
                }

                // Check branch consistency between sequential operations
                for pair in branch_checkers.windows(2) {
                    if let Some(tse) = TypestateChecker::check_branch_consistency(
                        &pair[0],
                        &pair[1],
                        decl.span.clone(),
                    ) {
                        errors.push(TypeError {
                            code: tse.code,
                            message: tse.message,
                            span: tse.span,
                            secondary: None,
                        });
                    }
                }
            }
        }
    }
    errors
}

/// T036: Run effect containment checks on functions and externs.
///
/// For each fn/extern with an `effects` clause, validates that the body's
/// actual effects are contained in the declared effect set.
fn run_effect_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let checker = EffectChecker::new();
    let mut errors = Vec::new();

    // Pass 1: Build effect map (name -> declared EffectSet) for call-graph inference.
    let effect_map = build_effect_map(source, &checker);

    for decl in &source.decls {
        match &decl.node {
            Decl::FnDef(f) => {
                let (declared, actual) = extract_effects_from_clauses(&f.clauses);
                if let Some(ref declared_set) = declared {
                    // Validate all effect names are known
                    for ee in checker.check_known(declared_set, &decl.span) {
                        errors.push(TypeError {
                            code: ee.code,
                            message: ee.message,
                            span: ee.span,
                            secondary: None,
                        });
                    }
                    // Check containment: actual subset of declared
                    if let Some(actual_set) = actual {
                        for ee in checker.check_containment(declared_set, &actual_set, &decl.span) {
                            errors.push(TypeError {
                                code: ee.code,
                                message: ee.message,
                                span: ee.span,
                                secondary: None,
                            });
                        }
                    }
                }

                // Pass 2: Call-graph effect inference. For each function call in
                // clause bodies, look up the callee's declared effects and check
                // they are a subset of the caller's declared effects.
                if let Some(ref declared_set) = declared {
                    let callee_effects = infer_callee_effects(&f.clauses, &effect_map);
                    for ee in checker.check_containment(declared_set, &callee_effects, &decl.span) {
                        // Rewrite the error message to include call-graph context
                        errors.push(TypeError {
                            code: ee.code,
                            message: ee.message,
                            span: ee.span,
                            secondary: None,
                        });
                    }
                }
            }
            Decl::Extern(e) => {
                let (declared, _) = extract_effects_from_clauses(&e.clauses);
                if let Some(declared_set) = declared {
                    for ee in checker.check_known(&declared_set, &decl.span) {
                        errors.push(TypeError {
                            code: ee.code,
                            message: ee.message,
                            span: ee.span,
                            secondary: None,
                        });
                    }
                }
            }
            Decl::Contract(c) => {
                let (declared, _) = extract_effects_from_clauses(&c.clauses);
                if let Some(declared_set) = declared {
                    for ee in checker.check_known(&declared_set, &decl.span) {
                        errors.push(TypeError {
                            code: ee.code,
                            message: ee.message,
                            span: ee.span,
                            secondary: None,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    errors
}

/// Build a map from function/contract/extern names to their declared (expanded)
/// effect sets. Used for call-graph-based effect inference in S002.
fn build_effect_map(
    source: &assura_parser::ast::SourceFile,
    checker: &EffectChecker,
) -> HashMap<String, EffectSet> {
    let mut map = HashMap::new();
    for decl in &source.decls {
        match &decl.node {
            Decl::FnDef(f) => {
                let (declared, _) = extract_effects_from_clauses(&f.clauses);
                if let Some(declared_set) = declared {
                    map.insert(f.name.clone(), checker.expand(&declared_set));
                }
            }
            Decl::Contract(c) => {
                let (declared, _) = extract_effects_from_clauses(&c.clauses);
                if let Some(declared_set) = declared {
                    map.insert(c.name.clone(), checker.expand(&declared_set));
                }
            }
            Decl::Extern(e) => {
                let (declared, _) = extract_effects_from_clauses(&e.clauses);
                if let Some(declared_set) = declared {
                    map.insert(e.name.clone(), checker.expand(&declared_set));
                }
            }
            Decl::Service(s) => {
                // Service operations may have effects
                for item in &s.items {
                    if let ServiceItem::Operation { name, clauses, .. } = item {
                        let (declared, _) = extract_effects_from_clauses(clauses);
                        if let Some(declared_set) = declared {
                            map.insert(name.clone(), checker.expand(&declared_set));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    map
}

/// Infer the union of all callee effects from function calls in clause bodies.
///
/// Scans `requires`, `ensures`, and `modifies` clause bodies for `Call` and
/// `MethodCall` expressions. For each call target that appears in the effect
/// map, unions that target's effects into the result.
fn infer_callee_effects(
    clauses: &[assura_parser::ast::Clause],
    effect_map: &HashMap<String, EffectSet>,
) -> EffectSet {
    let mut result = EffectSet::pure();
    for clause in clauses {
        if matches!(
            clause.kind,
            ClauseKind::Requires
                | ClauseKind::Ensures
                | ClauseKind::Modifies
                | ClauseKind::Invariant
                | ClauseKind::Rule
        ) {
            collect_call_effects(&clause.body, effect_map, &mut result);
        }
    }
    result
}

/// Recursively collect effects from function calls in an expression.
fn collect_call_effects(
    expr: &Expr,
    effect_map: &HashMap<String, EffectSet>,
    effects: &mut EffectSet,
) {
    match expr {
        Expr::Call { func, args } => {
            // Extract the function name from the call target
            if let Some(name) = extract_call_name(func)
                && let Some(callee_effects) = effect_map.get(&name)
            {
                for eff in callee_effects.iter() {
                    effects.insert(eff.to_string());
                }
            }
            // Also recurse into arguments
            for arg in args {
                collect_call_effects(arg, effect_map, effects);
            }
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            // Check if the method name is in the effect map
            if let Some(callee_effects) = effect_map.get(method.as_str()) {
                for eff in callee_effects.iter() {
                    effects.insert(eff.to_string());
                }
            }
            collect_call_effects(receiver, effect_map, effects);
            for arg in args {
                collect_call_effects(arg, effect_map, effects);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_call_effects(lhs, effect_map, effects);
            collect_call_effects(rhs, effect_map, effects);
        }
        Expr::UnaryOp { expr: inner, .. } => {
            collect_call_effects(inner, effect_map, effects);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_call_effects(cond, effect_map, effects);
            collect_call_effects(then_branch, effect_map, effects);
            if let Some(el) = else_branch {
                collect_call_effects(el, effect_map, effects);
            }
        }
        Expr::Block(items) | Expr::List(items) | Expr::Tuple(items) => {
            for item in items {
                collect_call_effects(item, effect_map, effects);
            }
        }
        Expr::Forall { body, domain, .. } | Expr::Exists { body, domain, .. } => {
            collect_call_effects(body, effect_map, effects);
            collect_call_effects(domain, effect_map, effects);
        }
        Expr::Old(inner)
        | Expr::Paren(inner)
        | Expr::Ghost(inner)
        | Expr::Field(inner, _)
        | Expr::Cast { expr: inner, .. } => {
            collect_call_effects(inner, effect_map, effects);
        }
        Expr::Index { expr: base, index } => {
            collect_call_effects(base, effect_map, effects);
            collect_call_effects(index, effect_map, effects);
        }
        Expr::Apply { args, .. } => {
            for arg in args {
                collect_call_effects(arg, effect_map, effects);
            }
        }
        Expr::Let { value, body, .. } => {
            collect_call_effects(value, effect_map, effects);
            collect_call_effects(body, effect_map, effects);
        }
        Expr::Match { scrutinee, arms } => {
            collect_call_effects(scrutinee, effect_map, effects);
            for arm in arms {
                collect_call_effects(&arm.body, effect_map, effects);
            }
        }
        // Leaf expressions have no sub-calls
        Expr::Ident(_) | Expr::Literal(_) | Expr::Raw(_) => {}
    }
}

/// Extract a function name from a Call target expression.
fn extract_call_name(func: &Expr) -> Option<String> {
    match func {
        Expr::Ident(name) => Some(name.clone()),
        Expr::Field(_, name) => Some(name.clone()),
        _ => None,
    }
}

/// Extract declared and actual effect sets from a list of clauses.
fn extract_effects_from_clauses(
    clauses: &[assura_parser::ast::Clause],
) -> (Option<EffectSet>, Option<EffectSet>) {
    let mut declared: Option<EffectSet> = None;
    let mut actual: Option<EffectSet> = None;

    for clause in clauses {
        if clause.kind == ClauseKind::Effects {
            // Extract effect names from the clause body
            let effects = extract_effect_names_from_expr(&clause.body);
            declared = Some(EffectSet::from_effect_names(effects));
        }
    }

    // Infer actual effects from other clauses (requires/ensures with IO references)
    let mut inferred = EffectSet::pure();
    for clause in clauses {
        if matches!(
            clause.kind,
            ClauseKind::Requires | ClauseKind::Ensures | ClauseKind::Modifies
        ) {
            infer_effects_from_expr(&clause.body, &mut inferred);
        }
    }
    if !inferred.is_pure() {
        actual = Some(inferred);
    }

    (declared, actual)
}

/// Extract effect names from an effects clause expression.
///
/// Effect names may be dot-separated (e.g., `console.read`) which the lexer
/// tokenizes as `["console", ".", "read"]`. This function joins them back
/// into single names before returning.
fn extract_effect_names_from_expr(expr: &Expr) -> Vec<String> {
    match expr {
        Expr::Ident(name) => vec![name.clone()],
        Expr::Raw(tokens) => {
            // Join dot-separated tokens: ["console", ".", "read"] -> "console.read"
            let filtered: Vec<&str> = tokens
                .iter()
                .map(|s| s.as_str())
                .filter(|t| *t != "," && *t != "{" && *t != "}")
                .collect();
            let mut names = Vec::new();
            let mut current = String::new();
            for tok in filtered {
                if tok == "." {
                    current.push('.');
                } else if current.ends_with('.') {
                    current.push_str(tok);
                } else {
                    if !current.is_empty() {
                        names.push(current);
                    }
                    current = tok.to_string();
                }
            }
            if !current.is_empty() {
                names.push(current);
            }
            names
        }
        Expr::Block(items) => items
            .iter()
            .flat_map(extract_effect_names_from_expr)
            .collect(),
        Expr::Field(base, field) => {
            // Field access expression: `console.read` parsed as Field(Ident("console"), "read")
            let mut base_names = extract_effect_names_from_expr(base);
            if let Some(last) = base_names.last_mut() {
                last.push('.');
                last.push_str(field);
            } else {
                base_names.push(field.clone());
            }
            base_names
        }
        _ => Vec::new(),
    }
}

/// Infer effects from expression content (look for IO-related identifiers).
///
/// Recognizes the full effect hierarchy from Section 3.6 of the spec:
/// - `io` sub-effects: console, file, network, process, env, time, random
/// - `mem` effects: alloc, dealloc, resize
/// - `panic` effects: panic, abort, unreachable
fn infer_effects_from_expr(expr: &Expr, effects: &mut EffectSet) {
    match expr {
        Expr::Ident(name) => {
            // IO sub-effects: console, file, network, socket, http, process, env, time, random
            let io_prefixes = [
                "console",
                "file",
                "stdin",
                "stdout",
                "stderr",
                "network",
                "socket",
                "http",
                "tcp",
                "udp",
                "process",
                "env",
                "time",
                "random",
                "rand",
                "print",
                "read_line",
                "write_file",
                "read_file",
                "open",
                "close",
                "flush",
                "seek",
            ];
            for prefix in &io_prefixes {
                if name.starts_with(prefix) || name == *prefix {
                    effects.insert("io".into());
                    return;
                }
            }
            // Memory effects
            if name.starts_with("alloc")
                || name.starts_with("dealloc")
                || name.starts_with("malloc")
                || name.starts_with("free")
                || name.starts_with("realloc")
                || name.starts_with("resize")
            {
                effects.insert("mem".into());
            }
            // Panic/divergence effects
            if name == "panic"
                || name == "abort"
                || name == "unreachable"
                || name == "exit"
                || name == "todo"
            {
                effects.insert("panic".into());
            }
        }
        Expr::Field(base, field) => {
            // Detect `obj.read()`, `obj.write()`, etc.
            let io_methods = [
                "read",
                "write",
                "flush",
                "close",
                "open",
                "seek",
                "send",
                "recv",
                "connect",
                "listen",
                "accept",
                "print",
                "println",
                "read_line",
            ];
            if io_methods.contains(&field.as_str()) {
                effects.insert("io".into());
            }
            infer_effects_from_expr(base, effects);
        }
        Expr::Call { func, args } => {
            infer_effects_from_expr(func, effects);
            for a in args {
                infer_effects_from_expr(a, effects);
            }
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            let io_methods = [
                "read",
                "write",
                "flush",
                "close",
                "open",
                "seek",
                "send",
                "recv",
                "connect",
                "listen",
                "accept",
                "print",
                "println",
                "read_line",
            ];
            if io_methods.contains(&method.as_str()) {
                effects.insert("io".into());
            }
            infer_effects_from_expr(receiver, effects);
            for a in args {
                infer_effects_from_expr(a, effects);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            infer_effects_from_expr(lhs, effects);
            infer_effects_from_expr(rhs, effects);
        }
        Expr::UnaryOp { expr, .. } | Expr::Paren(expr) | Expr::Old(expr) => {
            infer_effects_from_expr(expr, effects);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            infer_effects_from_expr(cond, effects);
            infer_effects_from_expr(then_branch, effects);
            if let Some(e) = else_branch {
                infer_effects_from_expr(e, effects);
            }
        }
        Expr::Block(items) | Expr::List(items) => {
            for item in items {
                infer_effects_from_expr(item, effects);
            }
        }
        _ => {}
    }
}

/// T047: Run taint checking using the file-level TaintChecker entry point.
fn run_taint_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    TaintChecker::check_file(source)
}

/// S003: Run information flow tracking on contracts and functions.
///
/// Assigns security labels to input parameters based on annotations
/// (`@secret`, `@confidential`, `@internal`) and traces information flow
/// through ensures clause expressions. Reports A08001 if secret-labeled
/// data flows to a public output, and A08004 for implicit flows through
/// branches where a secret condition influences a public assignment.
fn run_info_flow_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut errors = Vec::new();

    for decl in &source.decls {
        match &decl.node {
            Decl::Contract(c) => {
                errors.extend(check_contract_info_flow(c, &decl.span));
            }
            Decl::FnDef(f) => {
                errors.extend(check_fn_info_flow(f, &decl.span));
            }
            _ => {}
        }
    }

    // Run dependent type checks on type definitions with index parameters
    errors.extend(run_dependent_type_checks(source));

    errors
}

/// Check dependent type index validity on type and contract declarations.
fn run_dependent_type_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut dep_checker = DependentTypeChecker::new();
    let mut errors = Vec::new();

    // Pass 1: register enum types for finiteness checking
    for decl in &source.decls {
        if let Decl::EnumDef(e) = &decl.node {
            let variants: Vec<String> = e.variants.iter().map(|v| v.name.clone()).collect();
            dep_checker.register_enum(e.name.clone(), variants);
        }
    }

    // Pass 2: check type/contract declarations for dependent type annotations
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => c.clauses.as_slice(),
            Decl::FnDef(f) => f.clauses.as_slice(),
            _ => continue,
        };

        for clause in clauses {
            // Look for "dep_type" or "dependent" clause annotations
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "dep_type" || k == "dependent")
                && let Expr::Raw(tokens) = &clause.body
                && tokens.len() >= 2
            {
                let index_name = &tokens[0];
                let index_type = &tokens[1];
                // Validate the index kind
                for dte in dep_checker.validate_index(index_name, index_type, &decl.span) {
                    errors.push(TypeError {
                        code: dte.code,
                        message: dte.message,
                        span: dte.span,
                        secondary: None,
                    });
                }
                // Bind the index variable
                let dep_index = match index_type.as_str() {
                    "Nat" => DepIndex::Nat(index_name.clone()),
                    "Bool" => DepIndex::Bool(index_name.clone()),
                    other => DepIndex::Enum {
                        name: index_name.clone(),
                        enum_type: other.to_string(),
                    },
                };
                dep_checker.bind_index(index_name.clone(), dep_index.clone());

                // If there is a type expression argument, check it
                if tokens.len() >= 3 {
                    let base_type = parse_type_tokens(std::slice::from_ref(&tokens[2]));
                    let dep_type = DepType {
                        base: base_type.clone(),
                        indices: vec![dep_index],
                    };
                    dep_checker.register_dep_type(index_name.clone(), dep_type);
                }
            }

            // Check index expressions in type positions
            if let ClauseKind::Other(ref k) = clause.kind
                && k == "index_expr"
            {
                // Find the first bound index to check the expression against
                if let Some((_, idx)) = dep_checker.index_vars_ref().iter().next() {
                    for dte in dep_checker.check_index_expr(&clause.body, idx, &decl.span) {
                        errors.push(TypeError {
                            code: dte.code,
                            message: dte.message,
                            span: dte.span,
                            secondary: None,
                        });
                    }
                }
            }

            // Check index erasure in non-ghost contexts
            if matches!(clause.kind, ClauseKind::Ensures | ClauseKind::Requires) {
                let ghost_context = false;
                for dte in dep_checker.check_index_erasure(&clause.body, ghost_context, &decl.span)
                {
                    errors.push(TypeError {
                        code: dte.code,
                        message: dte.message,
                        span: dte.span,
                        secondary: None,
                    });
                }
            }
        }

        // Check dependent type equality in contracts with type annotations
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && k == "dep_type_eq"
                && let Expr::Raw(tokens) = &clause.body
                && tokens.len() >= 2
            {
                let name_a = &tokens[0];
                let name_b = &tokens[1];
                if let (Some(dt_a), Some(dt_b)) = (
                    dep_checker.dep_types_ref().get(name_a),
                    dep_checker.dep_types_ref().get(name_b),
                ) {
                    let a = dt_a.clone();
                    let b = dt_b.clone();
                    for dte in dep_checker.check_dep_type_eq(&a, &b, &decl.span) {
                        errors.push(TypeError {
                            code: dte.code,
                            message: dte.message,
                            span: dte.span,
                            secondary: None,
                        });
                    }
                }
            }
        }
    }

    errors
}

/// Check information flow for a contract declaration.
///
/// Scans input clauses for security label annotations (e.g., `secret`,
/// `confidential` in the type annotation). If any input is labeled secret,
/// ensures clauses are checked for flows to public outputs.
fn check_contract_info_flow(
    contract: &assura_parser::ast::ContractDecl,
    span: &Range<usize>,
) -> Vec<TypeError> {
    let mut checker = InfoFlowChecker::new();

    // Scan input clause params for security annotations
    for clause in &contract.clauses {
        if clause.kind == ClauseKind::Input {
            let mut _has = false;
            assign_labels_from_clause(&clause.body, &mut checker, &mut _has);
        }
        // Register purpose labels from "purpose" annotations
        if let ClauseKind::Other(ref k) = clause.kind
            && k == "purpose"
            && let Expr::Raw(tokens) = &clause.body
            && tokens.len() >= 2
        {
            checker.declare_purpose(tokens[0].clone(), tokens[1].clone());
        }
        // Register declassify annotations
        if let ClauseKind::Other(ref k) = clause.kind
            && k == "declassify"
        {
            let refs = collect_ident_references(&clause.body);
            for name in refs {
                checker.mark_declassify(name);
            }
        }
        // Register timing-sensitive functions
        if let ClauseKind::Other(ref k) = clause.kind
            && k == "timing_sensitive"
        {
            let refs = collect_ident_references(&clause.body);
            for name in refs {
                checker.register_timing_sensitive(name);
            }
        }
    }

    // Only check if at least one parameter has a security label
    if !checker.has_labels() {
        return Vec::new();
    }

    let mut errors = Vec::new();

    // Check ensures clauses for information flow violations using the checker's
    // built-in expression walker (handles implicit flows and covert channels)
    for clause in &contract.clauses {
        if clause.kind == ClauseKind::Ensures {
            for err in checker.check_expr(&clause.body, span) {
                errors.push(TypeError {
                    code: err.code,
                    message: err.message,
                    span: err.span,
                    secondary: None,
                });
            }
            // Also run the legacy per-expression check
            check_expr_info_flow(&clause.body, &checker, span, &mut errors);
        }
        // Check declassification annotations
        if clause.kind == ClauseKind::Ensures || clause.kind == ClauseKind::Requires {
            // Check for implicit declassification in assignments
            let refs = collect_ident_references(&clause.body);
            for name in &refs {
                if let Some(label) = checker.get_label(name) {
                    // Check covert channel through timing functions in ensures
                    if let Some(err) = checker.check_covert_channel(label, name, span) {
                        errors.push(TypeError {
                            code: err.code,
                            message: err.message,
                            span: err.span,
                            secondary: None,
                        });
                    }
                    // Check declassification
                    if let Some(err) =
                        checker.check_declassify(label, SecurityLabel::Public, false, span)
                    {
                        errors.push(TypeError {
                            code: err.code,
                            message: err.message,
                            span: err.span,
                            secondary: None,
                        });
                    }
                }
            }
        }
        // Use get_label and get_purpose for purpose-label mismatches
        if let ClauseKind::Other(ref k) = clause.kind
            && k == "purpose_check"
            && let Expr::Raw(tokens) = &clause.body
            && tokens.len() >= 2
        {
            let var_name = &tokens[0];
            let required_purpose = &tokens[1];
            if checker.get_label(var_name).is_some()
                && let Some(err) = checker.check_purpose_label(var_name, required_purpose, span)
            {
                errors.push(TypeError {
                    code: err.code,
                    message: err.message,
                    span: err.span,
                    secondary: None,
                });
            }
            // Also validate against registered purpose
            if let Some(purpose) = checker.get_purpose(var_name)
                && purpose != required_purpose.as_str()
            {
                errors.push(TypeError {
                    code: "A08003".into(),
                    message: format!(
                        "purpose mismatch for `{var_name}`: registered as `{purpose}`, \
                             required `{required_purpose}`"
                    ),
                    span: span.clone(),
                    secondary: None,
                });
            }
        }
    }

    errors
}

/// Check information flow for a function definition.
fn check_fn_info_flow(fn_def: &assura_parser::ast::FnDef, span: &Range<usize>) -> Vec<TypeError> {
    let mut checker = InfoFlowChecker::new();

    // Scan clause params for security annotations
    for clause in &fn_def.clauses {
        if clause.kind == ClauseKind::Input {
            let mut _has = false;
            assign_labels_from_clause(&clause.body, &mut checker, &mut _has);
        }
        // Register purpose, declassify, and timing-sensitive annotations
        if let ClauseKind::Other(ref k) = clause.kind
            && k == "purpose"
            && let Expr::Raw(tokens) = &clause.body
            && tokens.len() >= 2
        {
            checker.declare_purpose(tokens[0].clone(), tokens[1].clone());
        }
        if let ClauseKind::Other(ref k) = clause.kind
            && k == "declassify"
        {
            for name in collect_ident_references(&clause.body) {
                checker.mark_declassify(name);
            }
        }
        if let ClauseKind::Other(ref k) = clause.kind
            && k == "timing_sensitive"
        {
            for name in collect_ident_references(&clause.body) {
                checker.register_timing_sensitive(name);
            }
        }
    }

    // Also check function params for label annotations in type names
    for param in &fn_def.params {
        let label = infer_label_from_type_tokens(&param.ty);
        if label > SecurityLabel::Public {
            checker.declare(param.name.clone(), label);
        }
    }

    if !checker.has_labels() {
        return Vec::new();
    }

    let mut errors = Vec::new();

    for clause in &fn_def.clauses {
        if clause.kind == ClauseKind::Ensures {
            // Use the checker's built-in expression walker
            for err in checker.check_expr(&clause.body, span) {
                errors.push(TypeError {
                    code: err.code,
                    message: err.message,
                    span: err.span,
                    secondary: None,
                });
            }
            check_expr_info_flow(&clause.body, &checker, span, &mut errors);
        }
    }

    errors
}

/// Assign security labels from an input clause body.
///
/// Looks for patterns like `secret key: Bytes`, `confidential password: String`
/// where the security label is a keyword before the parameter name.
fn assign_labels_from_clause(expr: &Expr, checker: &mut InfoFlowChecker, has_any: &mut bool) {
    match expr {
        Expr::Raw(tokens) => {
            // Scan for label keywords followed by a param name
            let mut i = 0;
            while i < tokens.len() {
                let label = match tokens[i].as_str() {
                    "secret" | "restricted" => Some(SecurityLabel::Restricted),
                    "confidential" => Some(SecurityLabel::Confidential),
                    "internal" => Some(SecurityLabel::Internal),
                    "public" => Some(SecurityLabel::Public),
                    _ => None,
                };
                if let Some(label) = label
                    && label > SecurityLabel::Public
                    && let Some(name) = tokens.get(i + 1)
                    && name != ":"
                {
                    checker.declare(name.clone(), label);
                    *has_any = true;
                }
                i += 1;
            }
        }
        Expr::Block(items) => {
            for item in items {
                assign_labels_from_clause(item, checker, has_any);
            }
        }
        Expr::Call { args, .. } => {
            for arg in args {
                assign_labels_from_clause(arg, checker, has_any);
            }
        }
        _ => {}
    }
}

/// Infer a security label from type annotation tokens.
///
/// If the type annotation contains `secret`, `confidential`, or `internal`
/// as a modifier, returns the corresponding label.
fn infer_label_from_type_tokens(tokens: &[String]) -> SecurityLabel {
    for tok in tokens {
        match tok.as_str() {
            "secret" | "restricted" => return SecurityLabel::Restricted,
            "confidential" => return SecurityLabel::Confidential,
            "internal" => return SecurityLabel::Internal,
            _ => {}
        }
    }
    SecurityLabel::Public
}

/// Check an expression for information flow violations.
///
/// If a sub-expression has a high security label and it contributes to
/// a value that should be public (e.g., the `result` variable in an ensures
/// clause), report A08001.
fn check_expr_info_flow(
    expr: &Expr,
    checker: &InfoFlowChecker,
    span: &Range<usize>,
    errors: &mut Vec<TypeError>,
) {
    // Check if `result` is being assigned a value derived from secret data
    if let Expr::BinOp {
        lhs,
        rhs,
        op: BinOp::Eq,
        ..
    } = expr
    {
        // Pattern: result == expr or expr == result
        let (target, source) = if is_result_expr(lhs) {
            ("result", rhs.as_ref())
        } else if is_result_expr(rhs) {
            ("result", lhs.as_ref())
        } else {
            return;
        };

        let source_label = checker.infer_label(source);
        if source_label > SecurityLabel::Public
            && let Some(err) = checker.check_assignment(SecurityLabel::Public, source_label, span)
        {
            errors.push(TypeError {
                code: err.code,
                message: format!("information flow violation in `{target}`: {}", err.message),
                span: err.span,
                secondary: None,
            });
        }
    }

    // Check for implicit flows through if conditions
    if let Expr::If {
        cond, then_branch, ..
    } = expr
    {
        let cond_label = checker.infer_label(cond);
        if cond_label > SecurityLabel::Public {
            // Check if the branch body assigns to result or a public variable
            let branch_label = infer_branch_target_label(then_branch, checker);
            if let Some(err) = checker.check_implicit_flow(cond_label, branch_label, span) {
                errors.push(TypeError {
                    code: err.code,
                    message: err.message,
                    span: err.span,
                    secondary: None,
                });
            }
        }
    }
}

/// Check if an expression is `result` (the return value variable).
fn is_result_expr(expr: &Expr) -> bool {
    matches!(expr, Expr::Ident(name) if name == "result")
}

/// Infer the security label of a branch target.
///
/// If the branch references `result`, the target is Public (since result
/// flows out). Otherwise, use the checker's label inference.
fn infer_branch_target_label(expr: &Expr, checker: &InfoFlowChecker) -> SecurityLabel {
    // If the branch affects `result`, the target is public
    if contains_result_ref(expr) {
        SecurityLabel::Public
    } else {
        checker.infer_label(expr)
    }
}

/// Check if an expression tree contains a reference to `result`.
fn contains_result_ref(expr: &Expr) -> bool {
    match expr {
        Expr::Ident(name) => name == "result",
        Expr::BinOp { lhs, rhs, .. } => contains_result_ref(lhs) || contains_result_ref(rhs),
        Expr::Field(inner, _) | Expr::Old(inner) | Expr::Paren(inner) => contains_result_ref(inner),
        Expr::Call { func, args } => {
            contains_result_ref(func) || args.iter().any(contains_result_ref)
        }
        Expr::MethodCall { receiver, args, .. } => {
            contains_result_ref(receiver) || args.iter().any(contains_result_ref)
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            contains_result_ref(cond)
                || contains_result_ref(then_branch)
                || else_branch.as_ref().is_some_and(|e| contains_result_ref(e))
        }
        _ => false,
    }
}

/// T058: Run FFI boundary checks on extern declarations.
///
/// Only runs if at least one extern has explicit trust boundary annotations.
/// Without annotations, the checker would flag every extern as missing trust
/// info, which creates noise for files that don't use FFI boundary contracts.
fn run_ffi_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = FfiBoundaryChecker::new();
    let mut externs = Vec::new();
    let mut has_any_boundary = false;

    for decl in &source.decls {
        if let Decl::Extern(e) = &decl.node {
            let has_boundary = e.clauses.iter().any(
                |c| matches!(c.kind, ClauseKind::Other(ref k) if k == "trust" || k == "boundary"),
            );
            if has_boundary {
                has_any_boundary = true;
            }
            let has_contract = !e.clauses.is_empty();
            externs.push((
                e.name.clone(),
                has_boundary,
                has_contract,
                decl.span.clone(),
            ));

            // Register extern with trust boundary classification
            let boundary = if e.clauses.iter().any(|c| {
                matches!(&c.kind, ClauseKind::Other(k) if k == "trust")
                    && matches!(&c.body, Expr::Ident(v) if v == "trusted")
            }) {
                TrustBoundary::Trusted
            } else if e.clauses.iter().any(|c| {
                matches!(&c.kind, ClauseKind::Other(k) if k == "trust")
                    && matches!(&c.body, Expr::Ident(v) if v == "audited")
            }) {
                TrustBoundary::Audited
            } else {
                TrustBoundary::Untrusted
            };
            checker.register_extern(e.name.clone(), boundary);

            // Mark externs with requires/ensures as contracted
            let has_requires = e.clauses.iter().any(|c| c.kind == ClauseKind::Requires);
            let has_ensures = e.clauses.iter().any(|c| c.kind == ClauseKind::Ensures);
            if has_requires || has_ensures {
                checker.mark_contracted(e.name.clone());
            }
        }
    }

    // Only check if at least one extern uses boundary annotations
    if !has_any_boundary {
        return Vec::new();
    }

    let mut errors: Vec<TypeError> = checker
        .check_file(&externs)
        .into_iter()
        .map(|fe| TypeError {
            code: fe.code,
            message: fe.message,
            span: fe.span,
            secondary: None,
        })
        .collect();

    // Additional check: externs calling into unsafe without any contract clauses
    for decl in &source.decls {
        if let Decl::Extern(e) = &decl.node {
            let has_requires = e.clauses.iter().any(|c| c.kind == ClauseKind::Requires);
            let has_ensures = e.clauses.iter().any(|c| c.kind == ClauseKind::Ensures);
            // Externs with boundary annotations but no requires/ensures
            let has_boundary = e.clauses.iter().any(
                |c| matches!(c.kind, ClauseKind::Other(ref k) if k == "trust" || k == "boundary"),
            );
            if has_boundary && !has_requires && !has_ensures {
                errors.push(TypeError {
                    code: "A11005".into(),
                    message: format!(
                        "extern `{}` has trust boundary but no requires/ensures contracts; \
                         add preconditions and postconditions to validate the trust boundary",
                        e.name
                    ),
                    span: decl.span.clone(),
                    secondary: None,
                });
            }

            // Check unsafe confinement: functions with "unsafe" annotation
            let has_unsafe_ann = e
                .clauses
                .iter()
                .any(|c| matches!(&c.kind, ClauseKind::Other(k) if k == "unsafe"));
            let is_ffi_wrapper = has_boundary;
            for err in checker.check_unsafe_confinement(
                &e.name,
                is_ffi_wrapper,
                has_unsafe_ann,
                &decl.span,
            ) {
                errors.push(TypeError {
                    code: err.code,
                    message: err.message,
                    span: err.span,
                    secondary: None,
                });
            }
        }
    }

    // Check FFI call sites in function/contract clause bodies
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::FnDef(f) => f.clauses.as_slice(),
            Decl::Contract(c) => c.clauses.as_slice(),
            _ => continue,
        };
        for clause in clauses {
            if matches!(clause.kind, ClauseKind::Ensures | ClauseKind::Requires) {
                let refs = collect_ident_references(&clause.body);
                for callee in &refs {
                    // A reference to the callee in an ensures clause suggests
                    // the result is being validated (used in a postcondition).
                    let result_validated = clauses.iter().any(|c| {
                        c.kind == ClauseKind::Ensures && expr_references_var(&c.body, callee)
                    });
                    for err in checker.check_ffi_call(callee, result_validated, &decl.span) {
                        errors.push(TypeError {
                            code: err.code,
                            message: err.message,
                            span: err.span,
                            secondary: None,
                        });
                    }
                }
            }
        }
    }

    errors
}

/// T064: Run error propagation checks on functions that return error types.
fn run_error_propagation_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = ErrorPropagationChecker::new();
    let mut errors = Vec::new();

    // Pass 1: discover error policies from contracts with must_propagate annotations
    for decl in &source.decls {
        if let Decl::Contract(c) = &decl.node {
            let mut policy = ErrorPolicy::default();
            for clause in &c.clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && k == "must_propagate"
                    && let Expr::Raw(tokens) = &clause.body
                {
                    policy.must_propagate.extend(tokens.iter().cloned());
                }
                if let ClauseKind::Other(ref k) = clause.kind
                    && k == "must_check"
                    && let Expr::Raw(tokens) = &clause.body
                {
                    policy.must_check.extend(tokens.iter().cloned());
                }
                if let ClauseKind::Other(ref k) = clause.kind
                    && k == "must_not_mask"
                    && let Expr::Raw(tokens) = &clause.body
                    && tokens.len() >= 2
                {
                    policy
                        .must_not_mask
                        .push((tokens[0].clone(), tokens[1].clone()));
                }
                if let ClauseKind::Other(ref k) = clause.kind
                    && k == "must_preserve_detail"
                    && let Expr::Raw(tokens) = &clause.body
                {
                    policy.must_preserve_detail.extend(tokens.iter().cloned());
                }
            }
            if !policy.must_propagate.is_empty()
                || !policy.must_check.is_empty()
                || !policy.must_not_mask.is_empty()
                || !policy.must_preserve_detail.is_empty()
            {
                checker.register_policy(c.name.clone(), policy);
            }
        }
    }

    // Pass 2: check functions that catch errors for propagation violations
    for decl in &source.decls {
        if let Decl::FnDef(f) = &decl.node {
            // Check if return type is an error type
            let returns_error = f.return_ty.iter().any(|t| t == "Result" || t == "Error");
            if returns_error {
                for clause in &f.clauses {
                    if clause.kind == ClauseKind::Errors
                        && let Expr::Raw(tokens) = &clause.body
                    {
                        for error_code in tokens {
                            if checker.is_must_propagate(error_code) {
                                errors.push(TypeError {
                                    code: "A64001".into(),
                                    message: format!(
                                        "error code `{error_code}` in function `{}` must be \
                                         propagated, not caught",
                                        f.name
                                    ),
                                    span: decl.span.clone(),
                                    secondary: None,
                                });
                            }
                        }
                    }

                    // Check "catch" clauses for error action violations
                    if let ClauseKind::Other(ref k) = clause.kind
                        && k == "catch"
                        && let Expr::Raw(tokens) = &clause.body
                    {
                        let error_code = tokens.first().cloned().unwrap_or_default();
                        let action_kw = tokens.get(1).map(|s| s.as_str()).unwrap_or("");
                        let action = match action_kw {
                            "swallow" | "ignore" => ErrorAction::Swallow,
                            "translate" | "translate_to" => {
                                let target = tokens.get(2).cloned().unwrap_or_default();
                                ErrorAction::TranslateTo(target)
                            }
                            "propagate" | "rethrow" => ErrorAction::Propagate,
                            _ => ErrorAction::Handle,
                        };
                        if let Some(te) =
                            checker.validate_catch(&error_code, action, decl.span.clone())
                        {
                            errors.push(TypeError {
                                code: te.code,
                                message: te.message,
                                span: te.span,
                                secondary: None,
                            });
                        }
                    }

                    // Check function calls in ensures/requires for unchecked returns
                    if matches!(clause.kind, ClauseKind::Ensures | ClauseKind::Requires) {
                        let refs = collect_ident_references(&clause.body);
                        for fn_ref in &refs {
                            if let Some(te) =
                                checker.validate_unchecked_call(fn_ref, decl.span.clone())
                            {
                                errors.push(TypeError {
                                    code: te.code,
                                    message: te.message,
                                    span: te.span,
                                    secondary: None,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    errors
}

// ---------------------------------------------------------------------------
// Frame checking wiring (T045)
// ---------------------------------------------------------------------------

/// T045: Validate modifies clause structure.
///
/// The FrameChecker's scope validation (check_scope) is deferred until
/// expression-level name resolution is implemented, as the current type
/// environment doesn't contain local variables or clause-declared params,
/// causing false positives on valid code. The FrameChecker is already
/// used by the SMT crate's verify_clauses() for frame axiom injection,
/// which is its primary purpose.
fn run_frame_checks(
    source: &assura_parser::ast::SourceFile,
    _type_env: &TypeEnv,
    _symbols: &assura_resolve::SymbolTable,
) -> Vec<TypeError> {
    let mut errors = Vec::new();
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Extern(e) => &e.clauses,
            _ => continue,
        };
        let modifies_bodies: Vec<&Expr> = clauses
            .iter()
            .filter(|c| c.kind == ClauseKind::Modifies)
            .map(|c| &c.body)
            .collect();
        if modifies_bodies.is_empty() {
            continue;
        }
        let checker = FrameChecker::new(&modifies_bodies);
        // Validate that modifies clauses are non-empty (structural check)
        if checker.modified_set().is_empty() && !modifies_bodies.is_empty() {
            errors.push(TypeError {
                code: "A14001".into(),
                message: "empty modifies clause; list the variables this function may change"
                    .into(),
                span: decl.span.clone(),
                secondary: None,
            });
        }
    }
    errors
}

// ---------------------------------------------------------------------------
// Totality checking wiring (T053)
// ---------------------------------------------------------------------------

/// T053: Check termination of recursive functions via decreases measures.
///
/// Returns syntactically detected errors and pending SMT checks for cases
/// where the syntactic checker is inconclusive. The caller (CLI pipeline)
/// dispatches pending checks to assura-smt.
fn run_totality_checks(
    source: &assura_parser::ast::SourceFile,
) -> (Vec<TypeError>, Vec<PendingDecreaseCheck>) {
    let mut checker = TotalityChecker::new();
    let mut errors = Vec::new();
    let mut pending_smt = Vec::new();

    // Pre-register functions annotated as partial
    for decl in &source.decls {
        if let Decl::FnDef(f) = &decl.node
            && f.clauses
                .iter()
                .any(|c| matches!(&c.kind, ClauseKind::Other(s) if s == "partial"))
        {
            checker.mark_partial(f.name.clone());
        }
    }

    // Collect all function definitions for mutual recursion checking
    let mut fn_defs: Vec<(&assura_parser::ast::FnDef, &std::ops::Range<usize>)> = Vec::new();

    for decl in &source.decls {
        if let Decl::FnDef(f) = &decl.node {
            fn_defs.push((f, &decl.span));
            let (te_errors, te_pending) = checker.check_function_totality(f, &decl.span);
            for te in te_errors {
                errors.push(TypeError {
                    code: te.code,
                    message: te.message,
                    span: te.span,
                    secondary: None,
                });
            }
            pending_smt.extend(te_pending);
        }
    }

    // Check for mutual recursion across all function pairs
    if fn_defs.len() >= 2 {
        for te in checker.check_mutual_recursion(&fn_defs) {
            errors.push(TypeError {
                code: te.code,
                message: te.message,
                span: te.span,
                secondary: None,
            });
        }
    }

    (errors, pending_smt)
}

// ---------------------------------------------------------------------------
// Fixed-width integer checking wiring (T055)
// ---------------------------------------------------------------------------

/// T055: Detect potential integer overflow in fixed-width arithmetic.
fn run_fixed_width_checks(
    source: &assura_parser::ast::SourceFile,
    type_env: &TypeEnv,
) -> Vec<TypeError> {
    let mut errors = Vec::new();
    for decl in &source.decls {
        let (params, clauses): (&[assura_parser::ast::Param], &[_]) = match &decl.node {
            Decl::Contract(c) => (&[], c.clauses.as_slice()),
            Decl::FnDef(f) => (f.params.as_slice(), f.clauses.as_slice()),
            Decl::Extern(e) => (e.params.as_slice(), e.clauses.as_slice()),
            _ => continue,
        };

        // Build a per-decl checker with declared fixed-width bindings
        let mut fw_checker = FixedWidthChecker::new();
        for param in params {
            let ty_str = param.ty.join(" ");
            if let Some(fw_ty) = token_to_fixed_width_type(&ty_str) {
                fw_checker.declare(param.name.clone(), fw_ty);
            }
        }

        for clause in clauses {
            check_expr_fixed_width_full(
                &clause.body,
                type_env,
                &fw_checker,
                &decl.span,
                &mut errors,
            );
        }
    }
    errors
}

/// Check an expression using the full FixedWidthChecker (with bindings).
///
/// Calls `check_binop` and `check_division_by_zero` in addition to the
/// individual overflow/signedness/cast checks.
fn check_expr_fixed_width_full(
    expr: &Expr,
    type_env: &TypeEnv,
    fw_checker: &FixedWidthChecker,
    span: &std::ops::Range<usize>,
    errors: &mut Vec<TypeError>,
) {
    match expr {
        Expr::BinOp { lhs, op, rhs } => {
            check_expr_fixed_width_full(lhs, type_env, fw_checker, span, errors);
            check_expr_fixed_width_full(rhs, type_env, fw_checker, span, errors);

            if let Some(left_type) = infer_fixed_width_type_ext(lhs, type_env, fw_checker)
                && let Some(right_type) = infer_fixed_width_type_ext(rhs, type_env, fw_checker)
            {
                // Warn when mixing unsigned and signed in arithmetic (not just comparison)
                if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul)
                    && FixedWidthChecker::is_unsigned(&left_type)
                        != FixedWidthChecker::is_unsigned(&right_type)
                    && FixedWidthChecker::is_fixed_width(&left_type)
                    && FixedWidthChecker::is_fixed_width(&right_type)
                {
                    // already covered by check_binop's signedness check
                }
                // Use check_binop for combined overflow + signedness + div-by-zero
                for fwe in fw_checker.check_binop(op, &left_type, &right_type, rhs, span) {
                    errors.push(TypeError {
                        code: fwe.code,
                        message: fwe.message,
                        span: fwe.span,
                        secondary: None,
                    });
                }
            } else if let Some(left_type) = infer_fixed_width_type_ext(lhs, type_env, fw_checker) {
                // Even without right type, check division by zero
                if let Some(fwe) =
                    FixedWidthChecker::check_division_by_zero(op, rhs, &left_type, span)
                {
                    errors.push(TypeError {
                        code: fwe.code,
                        message: fwe.message,
                        span: fwe.span,
                        secondary: None,
                    });
                }
            }
        }
        Expr::Cast { expr: inner, ty } => {
            check_expr_fixed_width_full(inner, type_env, fw_checker, span, errors);
            if let Some(from_type) = infer_fixed_width_type_ext(inner, type_env, fw_checker)
                && let Some(to_ty) = token_to_fixed_width_type(ty)
                && let Some(fwe) = FixedWidthChecker::check_cast_safety(&from_type, &to_ty, span)
            {
                errors.push(TypeError {
                    code: fwe.code,
                    message: fwe.message,
                    span: fwe.span,
                    secondary: None,
                });
            }
        }
        Expr::UnaryOp { expr: inner, .. }
        | Expr::Old(inner)
        | Expr::Paren(inner)
        | Expr::Ghost(inner) => {
            check_expr_fixed_width_full(inner, type_env, fw_checker, span, errors);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            check_expr_fixed_width_full(cond, type_env, fw_checker, span, errors);
            check_expr_fixed_width_full(then_branch, type_env, fw_checker, span, errors);
            if let Some(e) = else_branch {
                check_expr_fixed_width_full(e, type_env, fw_checker, span, errors);
            }
        }
        Expr::Call { func, args } => {
            check_expr_fixed_width_full(func, type_env, fw_checker, span, errors);
            for a in args {
                check_expr_fixed_width_full(a, type_env, fw_checker, span, errors);
            }
        }
        Expr::Block(items) => {
            for item in items {
                check_expr_fixed_width_full(item, type_env, fw_checker, span, errors);
            }
        }
        _ => {}
    }
}

/// Infer fixed-width type using both type env and the checker's bindings.
fn infer_fixed_width_type_ext(
    expr: &Expr,
    type_env: &TypeEnv,
    fw_checker: &FixedWidthChecker,
) -> Option<Type> {
    match expr {
        Expr::Ident(name) => {
            // Check checker bindings first, then type env
            if let Some(ty) = fw_checker.get_type(name)
                && FixedWidthChecker::is_fixed_width(ty)
            {
                return Some(ty.clone());
            }
            if let Some(ty) = type_env.lookup(name)
                && FixedWidthChecker::is_fixed_width(ty)
            {
                return Some(ty.clone());
            }
            None
        }
        Expr::Cast { ty, .. } => token_to_fixed_width_type(ty),
        _ => None,
    }
}

/// Convert a type name token to a fixed-width Type.
fn token_to_fixed_width_type(ty: &str) -> Option<Type> {
    match ty {
        "U8" | "u8" => Some(Type::U8),
        "U16" | "u16" => Some(Type::U16),
        "U32" | "u32" => Some(Type::U32),
        "U64" | "u64" => Some(Type::U64),
        "I8" | "i8" => Some(Type::I8),
        "I16" | "i16" => Some(Type::I16),
        "I32" | "i32" => Some(Type::I32),
        "I64" | "i64" => Some(Type::I64),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Collection contract checks (T108)
// ---------------------------------------------------------------------------

/// Validate that contracts referencing standard collection operations
/// (sort, filter, map, reverse, deduplicate) declare postconditions
/// consistent with the operation's semantics.
fn run_collection_contract_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let cc = CollectionContracts::new();
    let mut errors = Vec::new();

    for decl in &source.decls {
        let (name, clauses) = match &decl.node {
            Decl::Contract(c) => (&c.name, &c.clauses),
            Decl::FnDef(f) => (&f.name, &f.clauses),
            _ => continue,
        };

        // Check if the contract/function name matches a known collection op
        let lower_name = name.to_lowercase();
        if let Some(cc_def) = cc.lookup(&lower_name) {
            // Verify length-preserving operations declare it
            if cc_def.preserves_length {
                let has_length_postcondition = clauses
                    .iter()
                    .any(|c| c.kind == ClauseKind::Ensures && expr_mentions_len(&c.body));
                if !has_length_postcondition {
                    errors.push(TypeError {
                        code: "A03007".into(),
                        message: format!(
                            "collection operation `{name}` preserves length; \
                             consider adding a `len(result) == len(input)` postcondition"
                        ),
                        span: decl.span.clone(),
                        secondary: None,
                    });
                }
            }
        }
    }

    errors
}

/// Check if an expression mentions `len` (used by collection contract checks).
fn expr_mentions_len(expr: &Expr) -> bool {
    match expr {
        Expr::Call { func, args } => {
            if let Expr::Ident(name) = func.as_ref()
                && name == "len"
            {
                return true;
            }
            args.iter().any(expr_mentions_len)
        }
        Expr::Ident(name) => name == "len",
        Expr::BinOp { lhs, rhs, .. } => expr_mentions_len(lhs) || expr_mentions_len(rhs),
        Expr::UnaryOp { expr, .. } => expr_mentions_len(expr),
        Expr::Paren(e) | Expr::Old(e) | Expr::Ghost(e) => expr_mentions_len(e),
        Expr::Field(e, _) => expr_mentions_len(e),
        Expr::Block(exprs) | Expr::List(exprs) => exprs.iter().any(expr_mentions_len),
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_mentions_len(cond)
                || expr_mentions_len(then_branch)
                || else_branch.as_ref().is_some_and(|e| expr_mentions_len(e))
        }
        Expr::Forall { body, domain, .. } | Expr::Exists { body, domain, .. } => {
            expr_mentions_len(body) || expr_mentions_len(domain)
        }
        Expr::Raw(tokens) => tokens.iter().any(|t| t == "len"),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Pattern exhaustiveness checking (T017)
// ---------------------------------------------------------------------------

/// A pattern in a match arm, used for exhaustiveness checking.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Pattern {
    /// Matches a specific enum variant by name.
    Variant(std::string::String),
    /// Wildcard `_` pattern that matches anything.
    Wildcard,
    /// Matches a specific literal value.
    Literal(assura_parser::ast::Literal),
}

/// Check whether a set of patterns exhaustively covers all variants of an enum.
///
/// Implements a simplified Maranget-style coverage check: collects the set of
/// variant names covered by the patterns (a `Wildcard` covers everything) and
/// compares against `enum_variants`.
///
/// Returns `None` if the patterns are exhaustive, or `Some(missing)` with the
/// list of uncovered variant names. The missing list preserves the declaration
/// order from `enum_variants`.
///
/// # Error code
///
/// When this returns `Some(_)`, the caller should report error **A10001**
/// (non-exhaustive match) and include the missing variants in the diagnostic.
pub(crate) fn check_exhaustiveness(
    patterns: &[Pattern],
    enum_variants: &[std::string::String],
) -> Option<Vec<std::string::String>> {
    // A wildcard covers all variants immediately.
    if patterns.iter().any(|p| matches!(p, Pattern::Wildcard)) {
        return None;
    }

    // Collect the set of variant names explicitly covered.
    let covered: std::collections::HashSet<&str> = patterns
        .iter()
        .filter_map(|p| match p {
            Pattern::Variant(name) => Some(name.as_str()),
            _ => None,
        })
        .collect();

    let missing: Vec<std::string::String> = enum_variants
        .iter()
        .filter(|v| !covered.contains(v.as_str()))
        .cloned()
        .collect();

    if missing.is_empty() {
        None
    } else {
        Some(missing)
    }
}

// ---------------------------------------------------------------------------
// Match exhaustiveness wiring (T017)
// ---------------------------------------------------------------------------

/// Walk all expressions in the source file and check match expressions
/// for exhaustiveness against known enum types in the symbol table.
fn run_match_exhaustiveness_checks(
    source: &assura_parser::ast::SourceFile,
    symbols: &assura_resolve::SymbolTable,
) -> Vec<TypeError> {
    let mut errors = Vec::new();

    // Build a map of enum name -> variant names
    let mut enum_variants: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for decl in &source.decls {
        if let Decl::EnumDef(e) = &decl.node {
            enum_variants.insert(
                e.name.clone(),
                e.variants.iter().map(|v| v.name.clone()).collect(),
            );
        }
    }

    // Walk all clause bodies looking for match expressions
    for decl in &source.decls {
        let clauses: &[assura_parser::ast::Clause] = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Extern(e) => &e.clauses,
            _ => continue,
        };
        for clause in clauses {
            check_match_exhaustiveness_expr(
                &clause.body,
                &decl.span,
                &enum_variants,
                symbols,
                &mut errors,
            );
        }
    }

    errors
}

/// Recursively walk an expression looking for match expressions.
fn check_match_exhaustiveness_expr(
    expr: &Expr,
    span: &std::ops::Range<usize>,
    enum_variants: &std::collections::HashMap<String, Vec<String>>,
    _symbols: &assura_resolve::SymbolTable,
    errors: &mut Vec<TypeError>,
) {
    match expr {
        Expr::Match { scrutinee, arms } => {
            // Recurse into scrutinee and arm bodies
            check_match_exhaustiveness_expr(scrutinee, span, enum_variants, _symbols, errors);
            for arm in arms {
                check_match_exhaustiveness_expr(&arm.body, span, enum_variants, _symbols, errors);
            }

            // Try to determine the enum type from the scrutinee
            if let Expr::Ident(name) = scrutinee.as_ref()
                && let Some(variants) = enum_variants.get(name)
            {
                let patterns: Vec<Pattern> = arms
                    .iter()
                    .map(|arm| match &arm.pattern {
                        assura_parser::ast::Pattern::Ident(n) => Pattern::Variant(n.clone()),
                        assura_parser::ast::Pattern::Wildcard => Pattern::Wildcard,
                        assura_parser::ast::Pattern::Literal(lit) => Pattern::Literal(lit.clone()),
                        assura_parser::ast::Pattern::Constructor { name, .. } => {
                            Pattern::Variant(name.clone())
                        }
                        assura_parser::ast::Pattern::Tuple(_) => Pattern::Wildcard,
                    })
                    .collect();

                if let Some(missing) = check_exhaustiveness(&patterns, variants) {
                    errors.push(TypeError {
                        code: "A10001".into(),
                        message: format!(
                            "non-exhaustive match: missing variants {}",
                            missing.join(", ")
                        ),
                        span: span.clone(),
                        secondary: None,
                    });
                }
            }

            // Even without known enum type, check that there is at least
            // a wildcard if we cannot determine the type
            let has_wildcard = arms
                .iter()
                .any(|arm| matches!(arm.pattern, assura_parser::ast::Pattern::Wildcard));
            let has_enum_coverage = if let Expr::Ident(name) = scrutinee.as_ref() {
                enum_variants.contains_key(name)
            } else {
                false
            };
            if !has_wildcard && !has_enum_coverage && !arms.is_empty() {
                // Warn about match without wildcard on unknown scrutinee type
                errors.push(TypeError {
                    code: "A10002".into(),
                    message: "match expression on unknown type has no wildcard `_` arm; \
                              consider adding a catch-all pattern"
                        .into(),
                    span: span.clone(),
                    secondary: None,
                });
            }
        }
        // Recurse into sub-expressions
        Expr::BinOp { lhs, rhs, .. } => {
            check_match_exhaustiveness_expr(lhs, span, enum_variants, _symbols, errors);
            check_match_exhaustiveness_expr(rhs, span, enum_variants, _symbols, errors);
        }
        Expr::UnaryOp { expr: e, .. }
        | Expr::Old(e)
        | Expr::Paren(e)
        | Expr::Ghost(e)
        | Expr::Field(e, _)
        | Expr::Cast { expr: e, .. } => {
            check_match_exhaustiveness_expr(e, span, enum_variants, _symbols, errors);
        }
        Expr::Call { func, args } => {
            check_match_exhaustiveness_expr(func, span, enum_variants, _symbols, errors);
            for a in args {
                check_match_exhaustiveness_expr(a, span, enum_variants, _symbols, errors);
            }
        }
        Expr::Apply { args, .. } => {
            for a in args {
                check_match_exhaustiveness_expr(a, span, enum_variants, _symbols, errors);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            check_match_exhaustiveness_expr(receiver, span, enum_variants, _symbols, errors);
            for a in args {
                check_match_exhaustiveness_expr(a, span, enum_variants, _symbols, errors);
            }
        }
        Expr::Index { expr: e, index } => {
            check_match_exhaustiveness_expr(e, span, enum_variants, _symbols, errors);
            check_match_exhaustiveness_expr(index, span, enum_variants, _symbols, errors);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            check_match_exhaustiveness_expr(cond, span, enum_variants, _symbols, errors);
            check_match_exhaustiveness_expr(then_branch, span, enum_variants, _symbols, errors);
            if let Some(e) = else_branch {
                check_match_exhaustiveness_expr(e, span, enum_variants, _symbols, errors);
            }
        }
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            check_match_exhaustiveness_expr(domain, span, enum_variants, _symbols, errors);
            check_match_exhaustiveness_expr(body, span, enum_variants, _symbols, errors);
        }
        Expr::Block(exprs) | Expr::List(exprs) => {
            for e in exprs {
                check_match_exhaustiveness_expr(e, span, enum_variants, _symbols, errors);
            }
        }
        Expr::Let { value, body, .. } => {
            check_match_exhaustiveness_expr(value, span, enum_variants, _symbols, errors);
            check_match_exhaustiveness_expr(body, span, enum_variants, _symbols, errors);
        }
        Expr::Tuple(elems) => {
            for e in elems {
                check_match_exhaustiveness_expr(e, span, enum_variants, _symbols, errors);
            }
        }
        Expr::Ident(_) | Expr::Literal(_) | Expr::Raw(_) => {}
    }
}

// ---------------------------------------------------------------------------
// Constant-time wiring (T059)
// ---------------------------------------------------------------------------

/// Scan for functions annotated with `constant_time` clause or `#[secret]`
/// parameter annotations and run the ConstantTimeChecker on their bodies.
fn run_constant_time_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut all_errors = Vec::new();

    for decl in &source.decls {
        let (clauses, params) = match &decl.node {
            Decl::FnDef(f) => (&f.clauses, f.params.as_slice()),
            Decl::Contract(c) => (&c.clauses, &[] as &[_]),
            Decl::Extern(e) => (&e.clauses, e.params.as_slice()),
            _ => continue,
        };

        // Check if function has a constant_time clause
        let has_ct = clauses
            .iter()
            .any(|c| matches!(&c.kind, ClauseKind::Other(k) if k == "constant_time"));
        if !has_ct {
            continue;
        }

        // Build checker: mark parameters with #[secret] or "secret" in type tokens
        let mut checker = ConstantTimeChecker::new();
        for param in params {
            let is_secret = param.ty.iter().any(|t| t == "secret" || t == "#[secret]");
            if is_secret {
                checker.mark_secret(param.name.clone());
            }
        }

        // Check all clause bodies for timing leaks
        for clause in clauses {
            for err in checker.check_expr(&clause.body, &decl.span) {
                all_errors.push(TypeError {
                    code: err.code,
                    message: err.message,
                    span: err.span,
                    secondary: None,
                });
            }
        }
    }

    all_errors
}

// ---------------------------------------------------------------------------
// Crypto conformance wiring (G001)
// ---------------------------------------------------------------------------

/// Scan for contracts/functions with `conforms`, `crypto`, or `spec` clause
/// annotations referencing a cryptographic algorithm. Extract algorithm name
/// and any key_size/nonce_size literals from clause bodies, then run the
/// CryptoConformanceChecker against the declared parameters.
fn run_crypto_conformance_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut all_errors = Vec::new();
    let mut checker = CryptoConformanceChecker::new();

    // Pre-register custom algorithm specs from "crypto_spec" clauses
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => c.clauses.as_slice(),
            Decl::FnDef(f) => f.clauses.as_slice(),
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && k == "crypto_spec"
                && let Expr::Raw(tokens) = &clause.body
                && !tokens.is_empty()
            {
                let name = tokens[0].trim_matches('"').to_string();
                let key_bits: Vec<u32> = tokens
                    .get(1)
                    .and_then(|s| s.parse().ok())
                    .into_iter()
                    .collect();
                let block_size = tokens.get(2).and_then(|s| s.parse().ok());
                let nonce_size = tokens.get(3).and_then(|s| s.parse().ok());
                let tag_size = tokens.get(4).and_then(|s| s.parse().ok());
                checker.register_spec(CryptoSpec {
                    name,
                    key_size_bits: key_bits,
                    block_size_bytes: block_size,
                    nonce_size_bytes: nonce_size,
                    tag_size_bytes: tag_size,
                });
            }
        }
    }

    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Extern(e) => &e.clauses,
            _ => continue,
        };

        // Look for conforms/crypto/spec clauses
        let mut algorithm: Option<String> = None;
        let mut key_size: Option<u32> = None;
        let mut nonce_size: Option<u32> = None;
        let mut has_tag_check = false;
        let mut nonce_source: Option<String> = None;
        let mut is_counter_nonce = false;
        let mut is_random_nonce = false;

        for clause in clauses {
            let kind_name = match &clause.kind {
                ClauseKind::Other(k) => k.as_str(),
                _ => continue,
            };

            match kind_name {
                "conforms" | "spec" | "crypto" => {
                    // Extract algorithm name from clause body
                    // Note: Literal::Str includes source quotes (e.g. `"AES-128-GCM"`)
                    if let Expr::Literal(assura_parser::ast::Literal::Str(name)) = &clause.body {
                        algorithm = Some(name.trim_matches('"').to_string());
                    } else if let Expr::Ident(name) = &clause.body {
                        algorithm = Some(name.clone());
                    } else if let Expr::Call { func, .. } = &clause.body {
                        if let Expr::Ident(name) = func.as_ref() {
                            algorithm = Some(name.clone());
                        }
                    } else if let Expr::Raw(tokens) = &clause.body
                        && let Some(t) = tokens.first()
                    {
                        // Fallback: extract from raw tokens (strip quotes)
                        let name = t.trim_matches('"').to_string();
                        if !name.is_empty() {
                            algorithm = Some(name);
                        }
                    }
                }
                "key_size" => {
                    if let Expr::Literal(assura_parser::ast::Literal::Int(s)) = &clause.body {
                        key_size = s.parse().ok();
                    } else if let Expr::Raw(tokens) = &clause.body
                        && let Some(t) = tokens.first()
                    {
                        key_size = t.parse().ok();
                    }
                }
                "nonce_size" => {
                    if let Expr::Literal(assura_parser::ast::Literal::Int(s)) = &clause.body {
                        nonce_size = s.parse().ok();
                    } else if let Expr::Raw(tokens) = &clause.body
                        && let Some(t) = tokens.first()
                    {
                        nonce_size = t.parse().ok();
                    }
                }
                "tag_verified" | "tag_check" => {
                    has_tag_check = true;
                }
                "nonce" => {
                    if let Expr::Ident(src) = &clause.body {
                        nonce_source = Some(src.clone());
                        is_counter_nonce = src.contains("counter") || src.contains("ctr");
                        is_random_nonce = src.contains("random") || src.contains("rng");
                    } else if let Expr::Raw(tokens) = &clause.body
                        && let Some(src) = tokens.first()
                    {
                        nonce_source = Some(src.clone());
                        is_counter_nonce = src.contains("counter") || src.contains("ctr");
                        is_random_nonce = src.contains("random") || src.contains("rng");
                    }
                }
                _ => {}
            }
        }

        // Run checks if an algorithm was declared
        if let Some(ref algo) = algorithm {
            if let Some(ks) = key_size {
                for err in checker.check_key_size(algo, ks, &decl.span) {
                    all_errors.push(TypeError {
                        code: err.code,
                        message: err.message,
                        span: err.span,
                        secondary: None,
                    });
                }
            }
            if let Some(ns) = nonce_size {
                for err in checker.check_nonce_size(algo, ns, &decl.span) {
                    all_errors.push(TypeError {
                        code: err.code,
                        message: err.message,
                        span: err.span,
                        secondary: None,
                    });
                }
            }
            if let Some(ref ns_src) = nonce_source {
                for err in checker.check_nonce_uniqueness(
                    ns_src,
                    is_counter_nonce,
                    is_random_nonce,
                    &decl.span,
                ) {
                    all_errors.push(TypeError {
                        code: err.code,
                        message: err.message,
                        span: err.span,
                        secondary: None,
                    });
                }
            }
            // Only check tag verification for decrypt-type operations
            let has_decrypt_clause = clauses.iter().any(
                |c| matches!(&c.kind, ClauseKind::Other(k) if k == "decrypt" || k == "decryption"),
            );
            if has_decrypt_clause {
                for err in checker.check_tag_verification(has_tag_check, &decl.span) {
                    all_errors.push(TypeError {
                        code: err.code,
                        message: err.message,
                        span: err.span,
                        secondary: None,
                    });
                }
            }
        }
    }

    all_errors
}

// ---------------------------------------------------------------------------
// Determinism wiring (T067)
// ---------------------------------------------------------------------------

/// Scan for functions with `pure` effect annotation and check that their
/// clause bodies do not reference non-deterministic sources.
fn run_determinism_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut all_errors = Vec::new();
    let mut checker = DeterminismChecker::new();

    for decl in &source.decls {
        let (fn_name, clauses) = match &decl.node {
            Decl::FnDef(f) => (f.name.as_str(), f.clauses.as_slice()),
            Decl::Contract(c) => (c.name.as_str(), c.clauses.as_slice()),
            _ => continue,
        };

        // Check if the function has a pure effects clause
        let is_pure = clauses.iter().any(|c| {
            c.kind == ClauseKind::Effects && matches!(&c.body, Expr::Ident(name) if name == "pure")
        });
        if !is_pure {
            continue;
        }

        checker.mark_deterministic(fn_name.to_string());

        // Register custom non-deterministic sources from annotations
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && k == "non_deterministic"
            {
                for name in collect_ident_references(&clause.body) {
                    checker.add_non_det_source(name);
                }
            }
        }

        // Collect all identifiers referenced in clause bodies
        let mut used_names = Vec::new();
        for clause in clauses {
            let refs = collect_ident_references(&clause.body);
            used_names.extend(refs);
        }

        for err in checker.check_fn_body(fn_name, &used_names, &decl.span) {
            all_errors.push(TypeError {
                code: err.code,
                message: err.message,
                span: err.span,
                secondary: None,
            });
        }

        // Check iteration over non-deterministic collections
        for name in &used_names {
            for err in checker.check_iteration(fn_name, name, &decl.span) {
                all_errors.push(TypeError {
                    code: err.code,
                    message: err.message,
                    span: err.span,
                    secondary: None,
                });
            }
        }
    }

    all_errors
}

// ---------------------------------------------------------------------------
// Memory safety wiring (T046)
// ---------------------------------------------------------------------------

/// Scan for functions with buffer/region parameters and validate memory
/// bounds annotations using the MemoryChecker.
fn run_memory_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut errors = Vec::new();

    // Per-function analysis: for each function with buffer-typed params,
    // check that its requires clauses include bounds checks.
    for decl in &source.decls {
        let (params, clauses) = match &decl.node {
            Decl::FnDef(f) => {
                // Skip axioms, lemmas, and ghost functions: they are
                // mathematical definitions without runtime semantics
                // and should not require bounds-checking annotations.
                if f.is_ghost || f.is_lemma {
                    continue;
                }
                // Axioms are parsed as FnDef with is_lemma=false but
                // use define/property clauses instead of requires/ensures.
                // Skip any function that has no requires AND no ensures.
                let has_runtime_contract = f
                    .clauses
                    .iter()
                    .any(|c| c.kind == ClauseKind::Requires || c.kind == ClauseKind::Ensures);
                if !has_runtime_contract {
                    continue;
                }
                (f.params.as_slice(), f.clauses.as_slice())
            }
            Decl::Extern(e) => (e.params.as_slice(), e.clauses.as_slice()),
            _ => continue,
        };

        let mut checker = MemoryChecker::new();
        let mut has_buffers = false;

        for param in params {
            let ty_str = param.ty.join(" ");
            if let Some(cap) = extract_capacity_annotation(&ty_str) {
                checker.register_buffer(param.name.clone(), cap);
                has_buffers = true;
            } else if ty_str.contains("Bytes") || ty_str.contains("Sequence") {
                checker.register_buffer(param.name.clone(), format!("{}.len", param.name));
                has_buffers = true;
            }
        }

        if !has_buffers {
            continue;
        }

        let requires_exprs: Vec<&Expr> = clauses
            .iter()
            .filter(|c| c.kind == ClauseKind::Requires)
            .map(|c| &c.body)
            .collect();

        // Register regions from "region" clauses
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && k == "region"
                && let Expr::Raw(tokens) = &clause.body
                && tokens.len() >= 4
            {
                // region name = lower..upper on buffer
                let region = MemoryRegion {
                    name: tokens[0].clone(),
                    lower: tokens.get(1).cloned().unwrap_or_default(),
                    upper: tokens.get(2).cloned().unwrap_or_default(),
                    buffer: tokens.get(3).cloned().unwrap_or_default(),
                };
                checker.register_region(region);
            }
        }

        for buf_name in checker.buffer_names() {
            // Any requires clause referencing the buffer counts as a
            // bounds constraint (the author is aware of the buffer).
            let has_any_constraint = requires_exprs
                .iter()
                .any(|expr| expr_references_var(expr, &buf_name));
            if has_any_constraint {
                continue;
            }
            if let Some(mem_err) =
                checker.check_bounds_in_requires(&buf_name, &requires_exprs, &decl.span)
            {
                errors.push(TypeError {
                    code: mem_err.code,
                    message: mem_err.message,
                    span: mem_err.span,
                    secondary: None,
                });
            }
        }

        // Check that regions reference valid buffers
        for mem_err in checker.check_region_buffers(&decl.span) {
            errors.push(TypeError {
                code: mem_err.code,
                message: mem_err.message,
                span: mem_err.span,
                secondary: None,
            });
        }

        // Check region containment from "contains" clauses
        let regions = checker.regions();
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && k == "contains"
                && let Expr::Raw(tokens) = &clause.body
                && tokens.len() >= 2
            {
                let sub_name = &tokens[0];
                let parent_name = &tokens[1];
                // Only check if both names match registered regions
                if (regions.iter().any(|r| r.name == *sub_name)
                    || regions.iter().any(|r| r.name == *parent_name))
                    && let Some(mem_err) =
                        checker.check_region_containment(sub_name, parent_name, &decl.span)
                {
                    errors.push(TypeError {
                        code: mem_err.code,
                        message: mem_err.message,
                        span: mem_err.span,
                        secondary: None,
                    });
                }
            }
        }
    }
    errors
}

/// Extract a capacity annotation from a type string like "Buffer<1024>" or
/// "Region<MAX_SIZE>".
fn extract_capacity_annotation(ty: &str) -> Option<String> {
    for prefix in &["Buffer", "Region", "FixedBuffer"] {
        if let Some(rest) = ty.strip_prefix(prefix)
            && let Some(inner) = rest.strip_prefix('<')
            && let Some(cap) = inner.strip_suffix('>')
        {
            return Some(cap.trim().to_string());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Secure erasure wiring (T060)
// ---------------------------------------------------------------------------

/// Scan for parameters annotated with `#[sensitive]` or `@sensitive` and
/// verify that functions handling sensitive data include erasure guarantees.
fn run_secure_erasure_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = SecureErasureChecker::new();
    let mut has_sensitive = false;

    for decl in &source.decls {
        let params = match &decl.node {
            Decl::FnDef(f) => f.params.as_slice(),
            Decl::Extern(e) => e.params.as_slice(),
            _ => continue,
        };

        for param in params {
            // Only `sensitive`/`#[sensitive]` triggers secure erasure.
            // `secret`/`#[secret]` is for constant-time checking (T059).
            let is_sensitive = param
                .ty
                .iter()
                .any(|t| t == "sensitive" || t == "#[sensitive]");
            if is_sensitive {
                checker.mark_sensitive(param.name.clone());
                has_sensitive = true;
            }
        }
    }

    if !has_sensitive {
        return Vec::new();
    }

    // Check that sensitive variables have scope-exit erasure
    let mut errors = Vec::new();
    let sensitive_names = checker.sensitive_names();
    for name in &sensitive_names {
        for decl in &source.decls {
            let (clauses, return_ty) = match &decl.node {
                Decl::FnDef(f) => (f.clauses.as_slice(), f.return_ty.as_slice()),
                Decl::Extern(e) => (e.clauses.as_slice(), e.return_ty.as_slice()),
                _ => continue,
            };

            // Look for zeroize/erase patterns in ensures clauses
            let has_erasure = clauses
                .iter()
                .any(|c| c.kind == ClauseKind::Ensures && expr_references_var(&c.body, name));
            if has_erasure {
                checker.mark_zeroized(name.clone());
            }

            // Check for copies of sensitive data to non-sensitive variables
            for clause in clauses {
                if clause.kind == ClauseKind::Ensures {
                    // Look for assignment patterns: target == source
                    if let Expr::BinOp {
                        lhs,
                        op: BinOp::Eq,
                        rhs,
                    } = &clause.body
                        && let Expr::Ident(src) = rhs.as_ref()
                        && src == name
                        && let Expr::Ident(tgt) = lhs.as_ref()
                    {
                        let tgt_is_sensitive = checker.sensitive_names().contains(tgt);
                        for err in checker.check_copy(name, tgt, tgt_is_sensitive, &decl.span) {
                            errors.push(TypeError {
                                code: err.code,
                                message: err.message,
                                span: err.span,
                                secondary: None,
                            });
                        }
                    }
                }
            }

            // Check if sensitive data is returned without @sensitive annotation
            let fn_return_is_sensitive = return_ty
                .iter()
                .any(|t| t == "sensitive" || t == "#[sensitive]");
            for err in checker.check_return(name, fn_return_is_sensitive, &decl.span) {
                errors.push(TypeError {
                    code: err.code,
                    message: err.message,
                    span: err.span,
                    secondary: None,
                });
            }
        }

        for err in checker.check_scope_exit(name, &(0..0)) {
            errors.push(TypeError {
                code: err.code,
                message: err.message,
                span: err.span,
                secondary: None,
            });
        }
    }

    // Final check: all sensitive variables should be erased
    for err in checker.check_all_erased(&(0..0)) {
        errors.push(TypeError {
            code: err.code,
            message: err.message,
            span: err.span,
            secondary: None,
        });
    }

    errors
}

// ---------------------------------------------------------------------------
// Interface contracts wiring (T062)
// ---------------------------------------------------------------------------

/// Scan for contracts with `implements` clauses and validate that all
/// required interface methods are present with correct signatures.
/// Extract an interface method declaration from a clause body expression.
///
/// Handles several forms:
/// - `Ident("method_name")` -> name only, no params/return
/// - `Call { func: Ident("f"), args }` -> name + param types from args
/// - `Raw(["f", "(", "Int", ")", "->", "Bool"])` -> name + parsed types
fn extract_interface_method(body: &Expr) -> Option<InterfaceMethod> {
    match body {
        Expr::Ident(name) => Some(InterfaceMethod {
            name: name.clone(),
            param_types: vec![],
            return_type: Type::Unknown,
            has_requires: false,
            has_ensures: false,
            no_reentrancy: false,
        }),
        Expr::Call { func, args } => {
            let name = match func.as_ref() {
                Expr::Ident(n) => n.clone(),
                _ => return None,
            };
            // Each arg in a method decl is typically a type identifier
            let param_types: Vec<Type> = args
                .iter()
                .map(|arg| match arg {
                    Expr::Ident(t) => parse_type_tokens(std::slice::from_ref(t)),
                    _ => Type::Unknown,
                })
                .collect();
            Some(InterfaceMethod {
                name,
                param_types,
                return_type: Type::Unknown,
                has_requires: false,
                has_ensures: false,
                no_reentrancy: false,
            })
        }
        Expr::Raw(tokens) => {
            // Try to extract method name from first token
            let name = tokens.first()?.clone();
            // Look for parameter types in parentheses
            let mut param_types = Vec::new();
            let mut return_type = Type::Unknown;
            if let Some(paren_start) = tokens.iter().position(|t| t == "(")
                && let Some(paren_end) = tokens.iter().position(|t| t == ")")
            {
                // Parse param types between ( and )
                let param_tokens = &tokens[paren_start + 1..paren_end];
                for chunk in param_tokens.split(|t| t == ",") {
                    if !chunk.is_empty() {
                        let owned: Vec<String> = chunk.to_vec();
                        param_types.push(parse_type_tokens(&owned));
                    }
                }
                // Look for -> return type after )
                if let Some(arrow_pos) = tokens[paren_end..].iter().position(|t| t == "->") {
                    let ret_tokens: Vec<String> = tokens[paren_end + arrow_pos + 1..].to_vec();
                    if !ret_tokens.is_empty() {
                        return_type = parse_type_tokens(&ret_tokens);
                    }
                }
            }
            Some(InterfaceMethod {
                name,
                param_types,
                return_type,
                has_requires: false,
                has_ensures: false,
                no_reentrancy: false,
            })
        }
        _ => None,
    }
}

fn run_interface_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = InterfaceChecker::new();
    let mut errors = Vec::new();

    // First pass: register all contracts that look like interfaces
    // (have `interface` as a clause kind or are named with Interface suffix).
    for decl in &source.decls {
        if let Decl::Contract(c) = &decl.node {
            let is_interface = c
                .clauses
                .iter()
                .any(|cl| matches!(&cl.kind, ClauseKind::Other(k) if k == "interface"));
            if is_interface {
                let methods: Vec<InterfaceMethod> = c
                    .clauses
                    .iter()
                    .filter(|cl| matches!(&cl.kind, ClauseKind::Other(k) if k == "method"))
                    .filter_map(|cl| extract_interface_method(&cl.body))
                    .collect();

                let extends: Vec<String> = c
                    .clauses
                    .iter()
                    .filter(|cl| matches!(&cl.kind, ClauseKind::Other(k) if k == "extends"))
                    .filter_map(|cl| {
                        if let Expr::Ident(name) = &cl.body {
                            Some(name.clone())
                        } else {
                            None
                        }
                    })
                    .collect();

                checker.register_interface(InterfaceContract {
                    name: c.name.clone(),
                    methods,
                    extends,
                });
            }
        }
    }

    // Second pass: check implementations
    for decl in &source.decls {
        if let Decl::Contract(c) = &decl.node {
            for clause in &c.clauses {
                if let ClauseKind::Other(k) = &clause.kind
                    && k == "implements"
                    && let Expr::Ident(iface_name) = &clause.body
                {
                    let impl_methods: Vec<InterfaceMethod> = c
                        .clauses
                        .iter()
                        .filter(|cl| matches!(&cl.kind, ClauseKind::Other(k) if k == "method"))
                        .filter_map(|cl| extract_interface_method(&cl.body))
                        .collect();

                    let method_names: Vec<String> =
                        impl_methods.iter().map(|m| m.name.clone()).collect();

                    // Register the implementation
                    checker.register_impl(c.name.clone(), iface_name.clone(), method_names.clone());

                    for err in checker.check_impl(&c.name, iface_name, &method_names, &decl.span) {
                        errors.push(TypeError {
                            code: err.code,
                            message: err.message,
                            span: err.span,
                            secondary: None,
                        });
                    }

                    // Check method signatures against the interface
                    for method in &impl_methods {
                        for err in checker.check_method_signature(
                            iface_name,
                            &method.name,
                            &method.param_types,
                            &method.return_type,
                            &decl.span,
                        ) {
                            errors.push(TypeError {
                                code: err.code,
                                message: err.message,
                                span: err.span,
                                secondary: None,
                            });
                        }

                        // Check reentrancy restrictions
                        let is_reentrant = c.clauses.iter().any(|cl| {
                            matches!(&cl.kind, ClauseKind::Other(k) if k == "reentrant")
                                && matches!(&cl.body, Expr::Ident(n) if n == &method.name)
                        });
                        for err in checker.check_reentrancy(
                            iface_name,
                            &method.name,
                            is_reentrant,
                            &decl.span,
                        ) {
                            errors.push(TypeError {
                                code: err.code,
                                message: err.message,
                                span: err.span,
                                secondary: None,
                            });
                        }
                    }
                }
            }
        }
    }

    errors
}

// ---------------------------------------------------------------------------
// Structural invariants wiring (T063)
// ---------------------------------------------------------------------------

/// Scan for types with structural invariant annotations and validate
/// that the invariant kind is applicable to the type's structure.
fn run_structural_invariant_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = StructuralInvariantChecker::new();
    let mut errors = Vec::new();

    for decl in &source.decls {
        match &decl.node {
            Decl::TypeDef(td) => {
                // Detect recursive types by checking if any field references
                // the type name itself.
                if let assura_parser::ast::TypeBody::Struct(fields) = &td.body {
                    let recursive_fields: Vec<String> = fields
                        .iter()
                        .filter(|f| f.ty.iter().any(|t| t == &td.name))
                        .map(|f| f.name.clone())
                        .collect();

                    if !recursive_fields.is_empty() {
                        checker.register_recursive_type(td.name.clone(), recursive_fields);
                    }
                }
            }
            Decl::Contract(c) => {
                // Look for structural_invariant clauses
                for clause in &c.clauses {
                    if let ClauseKind::Other(k) = &clause.kind
                        && k == "structural_invariant"
                    {
                        let kind = match &clause.body {
                            Expr::Ident(name) => match name.as_str() {
                                "sorted" => InvariantKind::Sorted { descending: false },
                                "acyclic" => InvariantKind::Acyclic,
                                "bst_ordering" => InvariantKind::BstOrdering,
                                other => InvariantKind::Custom(other.to_string()),
                            },
                            Expr::Call { func, .. } => {
                                if let Expr::Ident(name) = func.as_ref() {
                                    match name.as_str() {
                                        "tree_balance" => {
                                            InvariantKind::TreeBalance { max_diff: 1 }
                                        }
                                        "min_heap" => {
                                            InvariantKind::HeapProperty { min_heap: true }
                                        }
                                        "max_heap" => {
                                            InvariantKind::HeapProperty { min_heap: false }
                                        }
                                        other => InvariantKind::Custom(other.to_string()),
                                    }
                                } else {
                                    InvariantKind::Custom(format!("{:?}", clause.body))
                                }
                            }
                            _ => InvariantKind::Custom(format!("{:?}", clause.body)),
                        };

                        // Register the invariant for operation-preservation checking
                        checker.register_invariant(StructuralInvariant {
                            name: format!("{}_{}", c.name, kind),
                            type_name: c.name.clone(),
                            kind: kind.clone(),
                        });

                        for err in checker.check_invariant_applicability(&c.name, &kind, &decl.span)
                        {
                            errors.push(TypeError {
                                code: err.code,
                                message: err.message,
                                span: err.span,
                                secondary: None,
                            });
                        }
                    }

                    // Check that operations preserve registered invariants
                    if let ClauseKind::Other(k) = &clause.kind
                        && k == "modifies_structure"
                    {
                        let op_name = match &clause.body {
                            Expr::Ident(name) => name.as_str(),
                            _ => "unknown",
                        };
                        let has_preservation = c.clauses.iter().any(|cl| {
                            matches!(&cl.kind, ClauseKind::Other(k2) if k2 == "preserves_invariant")
                        });
                        for err in checker.check_operation_preserves(
                            &c.name,
                            op_name,
                            true,
                            has_preservation,
                            &decl.span,
                        ) {
                            errors.push(TypeError {
                                code: err.code,
                                message: err.message,
                                span: err.span,
                                secondary: None,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }

    errors
}

// ---------------------------------------------------------------------------
// Shared memory protocols wiring (T065)
// ---------------------------------------------------------------------------

/// Scan for functions with `shared` or `concurrent` annotations and
/// validate that access modes are declared correctly.
fn run_shared_mem_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut errors = Vec::new();

    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::FnDef(f) => &f.clauses,
            Decl::Contract(c) => &c.clauses,
            _ => continue,
        };

        let has_shared = clauses.iter().any(|c| {
            matches!(&c.kind, ClauseKind::Other(k) if k == "shared" || k == "concurrent" || k == "access_mode")
        });
        if !has_shared {
            continue;
        }

        let mut checker = SharedMemChecker::new();

        // Register access modes from clauses
        for clause in clauses {
            if let ClauseKind::Other(k) = &clause.kind
                && (k == "access_mode" || k == "shared")
                && let Expr::BinOp {
                    lhs,
                    op: BinOp::Implies,
                    rhs,
                } = &clause.body
                && let (Expr::Ident(obj), Expr::Ident(mode)) = (lhs.as_ref(), rhs.as_ref())
            {
                let access_mode = match mode.as_str() {
                    "exclusive" => AccessMode::Exclusive,
                    "shared_read" => AccessMode::SharedRead,
                    _ => AccessMode::None,
                };
                checker.set_mode(obj.clone(), access_mode);
            }
        }

        // Check modifies clauses reference objects with correct access
        for clause in clauses {
            if clause.kind == ClauseKind::Modifies {
                let modified = collect_ident_references(&clause.body);
                for name in &modified {
                    for err in checker.check_write(name, &decl.span) {
                        errors.push(TypeError {
                            code: err.code,
                            message: err.message,
                            span: err.span,
                            secondary: None,
                        });
                    }
                }
            }
            // Check read accesses in requires/ensures clauses
            if matches!(clause.kind, ClauseKind::Requires | ClauseKind::Ensures) {
                let refs = collect_ident_references(&clause.body);
                for name in &refs {
                    for err in checker.check_read(name, &decl.span) {
                        errors.push(TypeError {
                            code: err.code,
                            message: err.message,
                            span: err.span,
                            secondary: None,
                        });
                    }
                }
            }
            // Check for data race annotations
            if let ClauseKind::Other(k) = &clause.kind
                && k == "concurrent_access"
                && let Expr::Raw(tokens) = &clause.body
                && tokens.len() >= 3
            {
                let object = &tokens[0];
                let mode_a = match tokens[1].as_str() {
                    "exclusive" => AccessMode::Exclusive,
                    "shared_read" => AccessMode::SharedRead,
                    _ => AccessMode::None,
                };
                let mode_b = match tokens[2].as_str() {
                    "exclusive" => AccessMode::Exclusive,
                    "shared_read" => AccessMode::SharedRead,
                    _ => AccessMode::None,
                };
                for err in checker.check_data_race(object, mode_a, mode_b, &decl.span) {
                    errors.push(TypeError {
                        code: err.code,
                        message: err.message,
                        span: err.span,
                        secondary: None,
                    });
                }
            }
        }
    }

    errors
}

// ---------------------------------------------------------------------------
// Lock ordering wiring (T068)
// ---------------------------------------------------------------------------

/// Scan for lock ordering declarations and validate acquisition order.
fn run_lock_order_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = LockOrderChecker::new();
    let mut errors = Vec::new();

    // First pass: collect lock ordering declarations from blocks
    for decl in &source.decls {
        if let Decl::Block { kind, body, .. } = &decl.node
            && *kind == BlockKind::LockOrder
        {
            for (priority, clause) in body.iter().enumerate() {
                if let Expr::Ident(lock_name) = &clause.body {
                    checker.define_order(lock_name.clone(), priority as u32);
                }
            }
        }
    }

    // Second pass: check lock acquisitions in function bodies
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::FnDef(f) => &f.clauses,
            Decl::Contract(c) => &c.clauses,
            _ => continue,
        };

        for clause in clauses {
            if let ClauseKind::Other(k) = &clause.kind
                && (k == "acquires" || k == "locks")
            {
                let lock_names = collect_ident_references(&clause.body);
                for name in &lock_names {
                    // Check that the lock has a defined ordering
                    for err in checker.check_ordering_defined(name, &decl.span) {
                        errors.push(TypeError {
                            code: err.code,
                            message: err.message,
                            span: err.span,
                            secondary: None,
                        });
                    }
                    for err in checker.acquire(name, &decl.span) {
                        errors.push(TypeError {
                            code: err.code,
                            message: err.message,
                            span: err.span,
                            secondary: None,
                        });
                    }
                }
            }
            // Handle lock release clauses
            if let ClauseKind::Other(k) = &clause.kind
                && (k == "releases" || k == "unlocks")
            {
                let lock_names = collect_ident_references(&clause.body);
                for name in &lock_names {
                    for err in checker.release(name, &decl.span) {
                        errors.push(TypeError {
                            code: err.code,
                            message: err.message,
                            span: err.span,
                            secondary: None,
                        });
                    }
                }
            }
        }
    }

    errors
}

// ---------------------------------------------------------------------------
// Weak memory ordering wiring (G007, CONC.6)
// ---------------------------------------------------------------------------

/// Validate `ordering` clauses on atomic operations.
///
/// Checks:
/// - The ordering value is a recognized memory ordering keyword
///   (relaxed, acquire, release, acqrel, seq_cst)
/// - Contracts with `ordering: relaxed` that also have `ensures` clauses
///   depending on the value get A-CONC-016 warnings (relaxed read
///   without view check)
fn run_weak_memory_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    use assura_parser::ast::MemoryOrdering;
    let mut errors = Vec::new();

    for decl in &source.decls {
        let (name, clauses) = match &decl.node {
            Decl::Contract(c) => (c.name.as_str(), &c.clauses),
            Decl::FnDef(f) => (f.name.as_str(), &f.clauses),
            _ => continue,
        };

        let mut ordering_value: Option<MemoryOrdering> = None;
        let mut has_ensures = false;

        for clause in clauses {
            if clause.kind == ClauseKind::Ordering {
                // Extract the ordering value from the clause body
                let ordering_str = match &clause.body {
                    Expr::Ident(s) => Some(s.as_str()),
                    Expr::Raw(tokens) => tokens
                        .iter()
                        .find(|t| MemoryOrdering::parse(t).is_some())
                        .map(|t| t.as_str()),
                    _ => None,
                };
                if let Some(s) = ordering_str {
                    if let Some(ord) = MemoryOrdering::parse(s) {
                        ordering_value = Some(ord);
                    } else {
                        errors.push(TypeError {
                            code: "A-CONC-019".into(),
                            message: format!(
                                "unknown memory ordering `{s}` in `{name}`; \
                                 expected one of: relaxed, acquire, release, acqrel, seq_cst"
                            ),
                            span: decl.span.clone(),
                            secondary: None,
                        });
                    }
                }
            }
            if clause.kind == ClauseKind::Ensures {
                has_ensures = true;
            }
        }

        // A-CONC-016: relaxed read with ensures (value-dependent assertion)
        if ordering_value == Some(MemoryOrdering::Relaxed) && has_ensures {
            errors.push(TypeError {
                code: "A-CONC-016".into(),
                message: format!(
                    "relaxed ordering in `{name}` with ensures clause: \
                     value read with Relaxed may be stale; \
                     use Acquire for value-dependent assertions"
                ),
                span: decl.span.clone(),
                secondary: None,
            });
        }
    }

    errors
}

// ---------------------------------------------------------------------------
// Domain checker wiring: ~28 checkers from domain.rs
// ---------------------------------------------------------------------------

/// Scan for allocator/arena annotations and check allocation pairing.
fn run_allocator_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = AllocatorChecker::new();
    let mut has_alloc = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind {
                if k == "allocator" || k == "alloc" || k == "arena" {
                    has_alloc = true;
                    if let Expr::Ident(name) = &clause.body {
                        checker.record_alloc(name.clone(), None, decl.span.clone());
                    }
                }
                if (k == "dealloc" || k == "free")
                    && let Expr::Ident(name) = &clause.body
                    && let Some(err) = checker.record_free(name, decl.span.clone())
                {
                    return vec![err];
                }
            }
        }
    }
    // Wire arena lifecycle: declare arenas and track drop/use-after-drop
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind {
                if (k == "arena" || k == "declare_arena")
                    && let Expr::Ident(name) = &clause.body
                {
                    checker.declare_arena(name.clone());
                    has_alloc = true;
                }
                if (k == "drop_arena" || k == "arena_drop")
                    && let Expr::Ident(name) = &clause.body
                {
                    checker.drop_arena(name, decl.span.clone());
                }
            }
        }
    }
    if !has_alloc {
        return Vec::new();
    }
    // Check arena use-after-drop for all allocations
    let mut errors = Vec::new();
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if clause.kind == ClauseKind::Ensures {
                let refs = collect_ident_references(&clause.body);
                for name in &refs {
                    if let Some(err) = checker.check_arena_use(name, &decl.span) {
                        errors.push(err);
                    }
                }
            }
        }
    }
    errors.extend(checker.check_unpaired());
    errors
}

/// Scan for circular buffer declarations and validate indexing.
fn run_circular_buffer_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = CircularBufferChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "circular_buffer" || k == "ring_buffer")
            {
                found = true;
                match &clause.body {
                    Expr::Call { func, args } => {
                        if let Expr::Ident(name) = func.as_ref() {
                            let cap =
                                args.first().and_then(extract_int_literal).unwrap_or(256) as usize;
                            checker.declare(name.clone(), cap);
                        }
                    }
                    Expr::Ident(name) => {
                        checker.declare(name.clone(), 256);
                    }
                    _ => {
                        let kvs = extract_kv_pairs(&clause.body);
                        let name = kvs
                            .iter()
                            .find(|(k, _)| *k == "name" || *k == "buffer")
                            .and_then(|(_, v)| extract_ident(v))
                            .unwrap_or("unnamed")
                            .to_string();
                        let cap = kvs
                            .iter()
                            .find(|(k, _)| *k == "capacity" || *k == "size")
                            .and_then(|(_, v)| extract_int_literal(v))
                            .unwrap_or(256) as usize;
                        checker.declare(name, cap);
                    }
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    // Process push/pop operations and index checks via collected references
    let mut errors = Vec::new();
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind {
                if (k == "push" || k == "insert")
                    && let Expr::Ident(name) = &clause.body
                {
                    checker.push(name);
                }
                if (k == "pop" || k == "remove")
                    && let Expr::Ident(name) = &clause.body
                {
                    checker.pop(name);
                }
            }
            if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                let refs = collect_ident_references(&clause.body);
                for name in &refs {
                    if let Some(err) = checker.check_read(name, &decl.span) {
                        errors.push(err);
                    }
                    // Check index bounds for buffer references
                    if let Some(err) = checker.check_index(name, 0, &decl.span) {
                        errors.push(err);
                    }
                    // Check physical wrap for buffer access
                    if let Some(err) = checker.check_physical_wrap(name, 0, &decl.span) {
                        errors.push(err);
                    }
                }
            }
        }
    }
    // Check fullness and logical-to-physical mapping for declared buffers
    for (name, buf) in &checker.buffers {
        if buf.is_full() {
            errors.push(TypeError {
                code: "A23002".into(),
                message: format!("circular buffer `{name}` is full"),
                span: 0..1,
                secondary: None,
            });
        }
    }
    errors
}

/// Scan for callback/non-reentrant annotations and check re-entrancy.
fn run_callback_reentrancy_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = CallbackReentrancyChecker::new();
    let mut found = false;
    let mut max_depth_override: Option<usize> = None;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "non_reentrant" || k == "callback")
            {
                found = true;
                if let Expr::Ident(name) = &clause.body {
                    checker.mark_non_reentrant(name.clone(), decl.span.clone());
                }
            }
            // Extract max_depth configuration
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "max_depth" || k == "callback_depth")
                && let Some(depth) = extract_int_literal(&clause.body)
            {
                max_depth_override = Some(depth as usize);
            }
        }
    }
    // Apply max_depth configuration if specified
    if let Some(depth) = max_depth_override {
        checker = checker.with_max_depth(depth);
    }
    if !found {
        return Vec::new();
    }
    // Walk call references in clause bodies and simulate call/return for re-entrancy
    let mut errors = Vec::new();
    for decl in &source.decls {
        let (fn_name, clauses) = match &decl.node {
            Decl::FnDef(f) => (f.name.as_str(), &f.clauses),
            _ => continue,
        };
        // Enter the function scope
        let enter_errors = checker.enter_call(fn_name, &decl.span);
        errors.extend(enter_errors);
        // Check for callback registration in clause bodies
        for clause in clauses {
            if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                let refs = collect_ident_references(&clause.body);
                for name in &refs {
                    if let Some(err) = checker.check_register_callback(name, &decl.span) {
                        errors.push(err);
                    }
                }
            }
        }
        checker.exit_call();
    }
    // Include depth information in diagnostics if there are errors
    if !errors.is_empty() {
        let depth = checker.current_depth();
        if depth > 0 {
            errors.push(TypeError {
                code: "A24003".into(),
                message: format!("callback stack depth is {depth} at end of analysis"),
                span: 0..1,
                secondary: None,
            });
        }
    }
    errors
}

/// Scan for temporal deadline annotations and validate deadlines.
fn run_temporal_deadline_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = TemporalDeadlineChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "deadline" || k == "timeout" || k == "bounded_time")
            {
                found = true;
                // Extract deadline name and value from expression
                match &clause.body {
                    Expr::Call { func, args } => {
                        if let Expr::Ident(name) = func.as_ref() {
                            let ms =
                                args.first().and_then(extract_int_literal).unwrap_or(1000) as u64;
                            if let Some(err) = checker.enter_deadline(name.clone(), ms, &decl.span)
                            {
                                return vec![err];
                            }
                        }
                    }
                    Expr::Ident(name) => {
                        // bare identifier, use default 1000ms
                        if let Some(err) = checker.enter_deadline(name.clone(), 1000, &decl.span) {
                            return vec![err];
                        }
                    }
                    _ => {
                        // Try to extract kv pairs for named params
                        let kvs = extract_kv_pairs(&clause.body);
                        let name = kvs
                            .iter()
                            .find(|(k, _)| *k == "name")
                            .and_then(|(_, v)| extract_ident(v))
                            .unwrap_or("unnamed");
                        let ms = kvs
                            .iter()
                            .find(|(k, _)| *k == "ms" || *k == "timeout")
                            .and_then(|(_, v)| extract_int_literal(v))
                            .unwrap_or(1000) as u64;
                        if let Some(err) = checker.enter_deadline(name.to_string(), ms, &decl.span)
                        {
                            return vec![err];
                        }
                    }
                }
            }
            // Register operation bounds
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "worst_case" || k == "bound")
                && let Some((op, args)) = extract_call(&clause.body)
            {
                let ms = args.first().and_then(extract_int_literal).unwrap_or(0) as u64;
                checker.register_bound(op.to_string(), ms);
            }
        }
    }
    if !found {
        return Vec::new();
    }
    // Check operations within deadline contexts
    let mut errors = Vec::new();
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                let refs = collect_ident_references(&clause.body);
                for name in &refs {
                    if let Some(err) = checker.check_operation(name, &decl.span) {
                        // Include current deadline context in error
                        if let Some((dl_name, dl_ms)) = checker.current_deadline() {
                            errors.push(TypeError {
                                code: err.code.clone(),
                                message: format!(
                                    "{} (active deadline: `{dl_name}` {dl_ms}ms)",
                                    err.message
                                ),
                                span: err.span.clone(),
                                secondary: err.secondary.clone(),
                            });
                        } else {
                            errors.push(err);
                        }
                    }
                }
            }
            // Exit deadline scope for scope-exit annotations
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "exit_deadline" || k == "end_deadline")
            {
                checker.exit_deadline();
            }
        }
    }
    errors
}

/// Scan for binary format declarations and validate fields.
fn run_binary_format_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = BinaryFormatChecker::new();
    let mut found = false;
    let mut buffer_len: usize = 0;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind {
                if k == "binary_format" || k == "byte_layout" {
                    found = true;
                    // Extract buffer length from call syntax: binary_format(len)
                    if let Some((_, args)) = extract_call(&clause.body) {
                        if let Some(len) = args.first().and_then(extract_int_literal) {
                            buffer_len = len as usize;
                        }
                    } else if let Some(len) = extract_int_literal(&clause.body) {
                        buffer_len = len as usize;
                    }
                }
                if k == "field" {
                    found = true;
                    // Extract field from call syntax: field(name, offset, size)
                    // or from kv pairs: name = x, offset = y, size = z
                    match &clause.body {
                        Expr::Call { func, args } => {
                            if let Expr::Ident(name) = func.as_ref() {
                                let offset = args.first().and_then(extract_int_literal).unwrap_or(0)
                                    as usize;
                                let size =
                                    args.get(1).and_then(extract_int_literal).unwrap_or(1) as usize;
                                let endianness =
                                    args.get(2).and_then(extract_ident).map(|e| match e {
                                        "big" | "be" => Endianness::Big,
                                        "little" | "le" => Endianness::Little,
                                        _ => Endianness::Native,
                                    });
                                checker.add_field(BinaryField {
                                    name: name.clone(),
                                    offset,
                                    size,
                                    endianness,
                                    span: decl.span.clone(),
                                });
                            }
                        }
                        Expr::Ident(name) => {
                            checker.add_field(BinaryField {
                                name: name.clone(),
                                offset: 0,
                                size: 1,
                                endianness: None,
                                span: decl.span.clone(),
                            });
                        }
                        _ => {
                            let kvs = extract_kv_pairs(&clause.body);
                            let name = kvs
                                .iter()
                                .find(|(k, _)| *k == "name")
                                .and_then(|(_, v)| extract_ident(v))
                                .unwrap_or("unnamed")
                                .to_string();
                            let offset = kvs
                                .iter()
                                .find(|(k, _)| *k == "offset")
                                .and_then(|(_, v)| extract_int_literal(v))
                                .unwrap_or(0) as usize;
                            let size = kvs
                                .iter()
                                .find(|(k, _)| *k == "size")
                                .and_then(|(_, v)| extract_int_literal(v))
                                .unwrap_or(1) as usize;
                            let endianness = kvs
                                .iter()
                                .find(|(k, _)| *k == "endian" || *k == "endianness")
                                .and_then(|(_, v)| extract_ident(v))
                                .map(|e| match e {
                                    "big" | "be" => Endianness::Big,
                                    "little" | "le" => Endianness::Little,
                                    _ => Endianness::Native,
                                });
                            checker.add_field(BinaryField {
                                name,
                                offset,
                                size,
                                endianness,
                                span: decl.span.clone(),
                            });
                        }
                    }
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    checker.check_all(buffer_len)
}

/// Scan for bit-level format annotations and validate bit fields.
fn run_bit_level_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut container_bits: usize = 0;
    let mut checker: Option<BitLevelChecker> = None;
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind {
                if k == "bit_layout" || k == "bit_level" {
                    found = true;
                    // Extract container size: bit_layout(bits)
                    let bits = match &clause.body {
                        Expr::Call { func: _, args } => {
                            args.first().and_then(extract_int_literal).unwrap_or(64) as usize
                        }
                        Expr::Literal(_) => {
                            extract_int_literal(&clause.body).unwrap_or(64) as usize
                        }
                        _ => 64,
                    };
                    container_bits = bits;
                    checker = Some(BitLevelChecker::new(bits));
                }
                if k == "bit_field" {
                    found = true;
                    // Extract bit field: bit_field(name, offset, width) or bit_field(name, offset, width, cross_byte_ok)
                    if let Some(ref mut ch) = checker {
                        match &clause.body {
                            Expr::Call { func, args } => {
                                if let Expr::Ident(name) = func.as_ref() {
                                    let bit_offset =
                                        args.first().and_then(extract_int_literal).unwrap_or(0)
                                            as usize;
                                    let bit_width =
                                        args.get(1).and_then(extract_int_literal).unwrap_or(1)
                                            as usize;
                                    let cross_byte_ok = args
                                        .get(2)
                                        .and_then(extract_ident)
                                        .is_some_and(|v| v == "true");
                                    ch.add_field(BitField {
                                        name: name.clone(),
                                        bit_offset,
                                        bit_width,
                                        span: decl.span.clone(),
                                        cross_byte_ok,
                                    });
                                }
                            }
                            Expr::Ident(name) => {
                                ch.add_field(BitField {
                                    name: name.clone(),
                                    bit_offset: 0,
                                    bit_width: 1,
                                    span: decl.span.clone(),
                                    cross_byte_ok: false,
                                });
                            }
                            _ => {}
                        }
                    } else {
                        // No container declared yet, create default 64-bit
                        container_bits = 64;
                        let mut ch = BitLevelChecker::new(64);
                        if let Expr::Ident(name) = &clause.body {
                            ch.add_field(BitField {
                                name: name.clone(),
                                bit_offset: 0,
                                bit_width: 1,
                                span: decl.span.clone(),
                                cross_byte_ok: false,
                            });
                        }
                        checker = Some(ch);
                    }
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    match checker {
        Some(ch) => ch.check_all(container_bits),
        None => Vec::new(),
    }
}

/// Scan for string encoding annotations and validate.
fn run_string_encoding_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = StringEncodingChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "encoding" || k == "string_encoding" || k == "charset")
            {
                found = true;
                // Extract encoding from call syntax: encoding(name, enc_type)
                match &clause.body {
                    Expr::Call { func, args } => {
                        if let Expr::Ident(name) = func.as_ref() {
                            let enc = args
                                .first()
                                .and_then(extract_ident)
                                .map(parse_encoding)
                                .unwrap_or(StringEncoding::RawBytes);
                            checker.declare(name.clone(), enc);
                        }
                    }
                    Expr::Ident(name) => {
                        checker.declare(name.clone(), StringEncoding::RawBytes);
                    }
                    _ => {
                        let kvs = extract_kv_pairs(&clause.body);
                        let name = kvs
                            .iter()
                            .find(|(k, _)| *k == "name" || *k == "var")
                            .and_then(|(_, v)| extract_ident(v))
                            .unwrap_or("unnamed")
                            .to_string();
                        let enc = kvs
                            .iter()
                            .find(|(k, _)| *k == "encoding" || *k == "enc")
                            .and_then(|(_, v)| extract_ident(v))
                            .map(parse_encoding)
                            .unwrap_or(StringEncoding::RawBytes);
                        checker.declare(name, enc);
                    }
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    // Check for raw bytes used as strings, encoding compatibility, and truncation
    let mut errors = Vec::new();
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if clause.kind == ClauseKind::Ensures {
                let refs = collect_ident_references(&clause.body);
                for name in &refs {
                    if let Some(err) = checker.check_use_as_string(name, &decl.span) {
                        errors.push(err);
                    }
                    // Check encoding compatibility (target UTF-8 by default)
                    if let Some(err) =
                        checker.check_encoding_compat(name, &StringEncoding::Utf8, &decl.span)
                    {
                        errors.push(err);
                    }
                    // Check truncation at common byte boundaries
                    if let Some(err) = checker.check_truncation(name, 1, &decl.span) {
                        errors.push(err);
                    }
                }
            }
        }
    }
    errors
}

/// Parse a string encoding name to the enum.
fn parse_encoding(name: &str) -> StringEncoding {
    match name {
        "utf8" | "UTF8" | "utf-8" | "UTF-8" => StringEncoding::Utf8,
        "utf16le" | "UTF16LE" | "utf-16le" => StringEncoding::Utf16Le,
        "utf16be" | "UTF16BE" | "utf-16be" => StringEncoding::Utf16Be,
        "ascii" | "ASCII" => StringEncoding::Ascii,
        "latin1" | "LATIN1" | "iso-8859-1" => StringEncoding::Latin1,
        _ => StringEncoding::RawBytes,
    }
}

/// Scan for checksum annotations and validate verification order.
fn run_checksum_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = ChecksumChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind {
                if k == "checksum" || k == "crc" || k == "hash" {
                    found = true;
                    // Extract checksum params: checksum(name, algorithm, start, end)
                    match &clause.body {
                        Expr::Call { func, args } => {
                            if let Expr::Ident(name) = func.as_ref() {
                                let algo = args
                                    .first()
                                    .and_then(extract_ident)
                                    .map(parse_checksum_algorithm)
                                    .unwrap_or(ChecksumAlgorithm::Crc32);
                                let start =
                                    args.get(1).and_then(extract_int_literal).unwrap_or(0) as usize;
                                let end = args.get(2).and_then(extract_int_literal).unwrap_or(1024)
                                    as usize;
                                checker.declare_region(name.clone(), algo, start, end);
                            }
                        }
                        Expr::Ident(name) => {
                            checker.declare_region(name.clone(), ChecksumAlgorithm::Crc32, 0, 1024);
                        }
                        _ => {
                            let kvs = extract_kv_pairs(&clause.body);
                            let name = kvs
                                .iter()
                                .find(|(k, _)| *k == "name" || *k == "region")
                                .and_then(|(_, v)| extract_ident(v))
                                .unwrap_or("unnamed")
                                .to_string();
                            let algo = kvs
                                .iter()
                                .find(|(k, _)| *k == "algorithm" || *k == "algo")
                                .and_then(|(_, v)| extract_ident(v))
                                .map(parse_checksum_algorithm)
                                .unwrap_or(ChecksumAlgorithm::Crc32);
                            let start = kvs
                                .iter()
                                .find(|(k, _)| *k == "start")
                                .and_then(|(_, v)| extract_int_literal(v))
                                .unwrap_or(0) as usize;
                            let end = kvs
                                .iter()
                                .find(|(k, _)| *k == "end")
                                .and_then(|(_, v)| extract_int_literal(v))
                                .unwrap_or(1024) as usize;
                            checker.declare_region(name, algo, start, end);
                        }
                    }
                }
                if (k == "verify_checksum" || k == "verified")
                    && let Expr::Ident(name) = &clause.body
                {
                    checker.mark_verified(name);
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    // Check for use before verification, algorithm match, and range coverage
    let mut errors = Vec::new();
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                let refs = collect_ident_references(&clause.body);
                for name in &refs {
                    if let Some(err) = checker.check_use_before_verify(name, &decl.span) {
                        errors.push(err);
                    }
                    // Check algorithm consistency (verify declared matches expected)
                    if let Some(err) =
                        checker.check_algorithm_match(name, &ChecksumAlgorithm::Crc32, &decl.span)
                    {
                        errors.push(err);
                    }
                    // Check range coverage (verify checksum covers data range)
                    if let Some(err) = checker.check_range_coverage(name, 0, 1024, &decl.span) {
                        errors.push(err);
                    }
                }
            }
        }
    }
    errors
}

/// Parse a checksum algorithm name to the enum.
fn parse_checksum_algorithm(name: &str) -> ChecksumAlgorithm {
    match name {
        "crc32" | "CRC32" | "crc" => ChecksumAlgorithm::Crc32,
        "adler32" | "ADLER32" | "adler" => ChecksumAlgorithm::Adler32,
        "sha256" | "SHA256" | "sha-256" => ChecksumAlgorithm::Sha256,
        "sha512" | "SHA512" | "sha-512" => ChecksumAlgorithm::Sha512,
        "md5" | "MD5" => ChecksumAlgorithm::Md5,
        _ => ChecksumAlgorithm::Custom(name.to_string()),
    }
}

/// Scan for protocol grammar/state machine annotations and validate transitions.
fn run_protocol_grammar_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker: Option<ProtocolGrammarChecker> = None;
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind {
                if k == "protocol" || k == "state_machine" || k == "rfc" {
                    found = true;
                    // Extract initial state from expression
                    let initial = extract_ident(&clause.body).unwrap_or("init").to_string();
                    if checker.is_none() {
                        checker = Some(ProtocolGrammarChecker::new(initial));
                    }
                }
                // Register states
                if (k == "state" || k == "protocol_state")
                    && let Some(name) = extract_ident(&clause.body)
                    && let Some(ref mut ch) = checker
                {
                    ch.add_state(name.to_string());
                }
                // Register transitions: transition(from, msg, to)
                if k == "transition"
                    && let Some((from, args)) = extract_call(&clause.body)
                    && args.len() >= 2
                    && let Some(ref mut ch) = checker
                {
                    let msg = extract_ident(&args[0]).unwrap_or("unknown").to_string();
                    let to = extract_ident(&args[1]).unwrap_or("unknown").to_string();
                    ch.add_transition(from.to_string(), to, msg);
                }
                // Register required fields: required_fields(msg, [field1, field2])
                if (k == "required_fields" || k == "required")
                    && let Some((msg, args)) = extract_call(&clause.body)
                    && let Some(ref mut ch) = checker
                {
                    let field_names: Vec<String> = args
                        .iter()
                        .filter_map(|a| extract_ident(a).map(String::from))
                        .collect();
                    ch.add_required_fields(msg.to_string(), field_names);
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    let checker = match checker {
        Some(c) => c,
        None => return Vec::new(),
    };
    // Validate message sends, transitions, and required fields
    let mut checker = checker;
    let mut errors = Vec::new();
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "send" || k == "message")
                && let Some(msg) = extract_ident(&clause.body)
            {
                if let Some(err) = checker.check_send(msg, &decl.span) {
                    errors.push(err);
                }
                // Perform state transition for the message
                if let Some(err) = checker.transition(msg, &decl.span) {
                    errors.push(err);
                }
                // Check required fields for the message (none provided by default)
                let field_errs = checker.check_required_fields(msg, &[], &decl.span);
                errors.extend(field_errs);
            }
        }
    }
    errors
}

/// Scan for opaque function declarations and check contracts.
fn run_opaque_function_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = OpaqueFunctionChecker::new();
    let mut found = false;
    for decl in &source.decls {
        match &decl.node {
            Decl::FnDef(f) => {
                for clause in &f.clauses {
                    if let ClauseKind::Other(ref k) = clause.kind
                        && k == "opaque"
                    {
                        found = true;
                        let has_contract = f
                            .clauses
                            .iter()
                            .any(|c| matches!(c.kind, ClauseKind::Requires | ClauseKind::Ensures));
                        checker.declare_opaque(f.name.clone(), has_contract, decl.span.clone());
                    }
                }
            }
            Decl::Contract(c) => {
                for clause in &c.clauses {
                    if let ClauseKind::Other(ref k) = clause.kind
                        && k == "opaque"
                    {
                        found = true;
                    }
                }
            }
            _ => {}
        }
    }
    if !found {
        return Vec::new();
    }
    // Check that opaque functions called without contracts are flagged
    let mut errors = Vec::new();
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            // Handle proof context and reveal annotations
            if let ClauseKind::Other(ref k) = clause.kind {
                if k == "proof" || k == "proof_context" {
                    checker.enter_proof();
                }
                if k == "end_proof" {
                    checker.exit_proof();
                }
                if k == "reveal"
                    && let Expr::Ident(fn_name) = &clause.body
                    && let Some(err) = checker.reveal(fn_name, &decl.span)
                {
                    errors.push(err);
                }
            }
            if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                let refs = collect_ident_references(&clause.body);
                for name in &refs {
                    if let Some(err) = checker.check_call(name, &decl.span) {
                        errors.push(err);
                    }
                    // Check body access for opaque functions
                    if checker.is_opaque(name)
                        && let Some(mut err) = checker.check_body_access(name, &decl.span)
                    {
                        err.secondary = checker.opaque_span(name).map(|s| {
                            (s.clone(), format!("opaque function `{name}` declared here"))
                        });
                        errors.push(err);
                    }
                }
            }
        }
    }
    errors
}

/// Scan for crash recovery / WAL annotations and validate write ordering.
fn run_crash_recovery_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = CrashRecoveryChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind {
                if k == "wal" || k == "crash_recovery" || k == "write_ahead" {
                    found = true;
                    if let Some(id) = extract_ident(&clause.body) {
                        checker.begin_write(id.to_string());
                    }
                }
                if (k == "write_data" || k == "data_write")
                    && let Some(id) = extract_ident(&clause.body)
                {
                    checker.write_data(id);
                }
                if (k == "write_wal" || k == "wal_write")
                    && let Some(id) = extract_ident(&clause.body)
                {
                    checker.write_wal(id);
                }
                if (k == "fsync" || k == "flush")
                    && let Some(id) = extract_ident(&clause.body)
                {
                    checker.fsync(id);
                }
                if k == "commit"
                    && let Some(id) = extract_ident(&clause.body)
                {
                    checker.commit(id);
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    checker.check_all()
}

/// Scan for page cache annotations.
fn run_page_cache_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker: Option<PageCacheChecker> = None;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "page_cache" || k == "buffer_pool" || k == "cache_policy")
            {
                // Extract capacity from annotation body
                let capacity = match &clause.body {
                    Expr::Call { args, .. } => {
                        args.first().and_then(extract_int_literal).unwrap_or(1024) as usize
                    }
                    Expr::Literal(assura_parser::ast::Literal::Int(s)) => {
                        s.parse::<usize>().unwrap_or(1024)
                    }
                    _ => {
                        let kvs = extract_kv_pairs(&clause.body);
                        kvs.iter()
                            .find(|(k, _)| *k == "capacity" || *k == "size" || *k == "max_pages")
                            .and_then(|(_, v)| extract_int_literal(v))
                            .unwrap_or(1024) as usize
                    }
                };
                if checker.is_none() {
                    checker = Some(PageCacheChecker::new(capacity));
                }
            }
            // Extract page operations from requires/ensures clauses
            if let Some(ch) = checker.as_mut()
                && matches!(
                    clause.kind,
                    ClauseKind::Requires | ClauseKind::Ensures | ClauseKind::Other(_)
                )
            {
                page_cache_scan_expr(&clause.body, ch);
            }
        }
    }
    match checker {
        Some(ch) => ch.check_capacity(),
        None => Vec::new(),
    }
}

/// Scan an expression for page cache operations (load_page, pin, dirty, evict, page_count).
fn page_cache_scan_expr(expr: &Expr, checker: &mut PageCacheChecker) {
    if let Some((name, args)) = extract_call(expr) {
        let page_id = args.first().and_then(extract_int_literal).unwrap_or(0) as u64;
        match name {
            "load_page" | "load" | "fetch_page" => checker.load_page(page_id),
            "pin" | "pin_page" => checker.pin(page_id),
            "unpin" | "unpin_page" => checker.unpin(page_id),
            "mark_dirty" | "dirty" => checker.mark_dirty(page_id),
            "flush" | "flush_page" => checker.flush(page_id),
            "evict" | "evict_page" => {
                checker.evict(page_id);
            }
            _ => {}
        }
    }
}

/// Scan for MVCC/snapshot isolation annotations.
fn run_mvcc_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = MvccChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "mvcc" || k == "snapshot_isolation" || k == "serializable")
            {
                found = true;
                // Extract transaction operations from annotation body
                mvcc_scan_expr(&clause.body, &mut checker);
            }
            // Also scan requires/ensures for transaction operations
            if found && matches!(clause.kind, ClauseKind::Requires | ClauseKind::Ensures) {
                mvcc_scan_expr(&clause.body, &mut checker);
            }
        }
    }
    if !found {
        return Vec::new();
    }
    let mut errors = checker.check_write_conflicts();
    // Check snapshot read isolation for referenced keys
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if clause.kind == ClauseKind::Ensures {
                let refs = collect_ident_references(&clause.body);
                for name in &refs {
                    if let Some(err) = checker.check_snapshot_read(name, 1) {
                        errors.push(err);
                    }
                }
            }
        }
    }
    // Check phantom reads for the first transaction
    errors.extend(checker.check_phantom(1));
    errors
}

/// Scan an expression for MVCC operations (begin_txn, write, commit).
fn mvcc_scan_expr(expr: &Expr, checker: &mut MvccChecker) {
    if let Some((name, args)) = extract_call(expr) {
        match name {
            "begin_txn" | "begin" | "start_transaction" => {
                checker.begin_txn();
            }
            "write" | "write_version" | "put" => {
                let key = args
                    .first()
                    .and_then(extract_ident)
                    .unwrap_or("default")
                    .to_string();
                let txn_id = args.get(1).and_then(extract_int_literal).unwrap_or(1) as u64;
                checker.write_version(key, txn_id);
            }
            "commit" | "commit_txn" => {
                let txn_id = args.first().and_then(extract_int_literal).unwrap_or(1) as u64;
                checker.commit_txn(txn_id);
            }
            _ => {}
        }
    }
    // Scan sub-expressions in blocks/lists
    match expr {
        Expr::Block(exprs) | Expr::List(exprs) => {
            for e in exprs {
                mvcc_scan_expr(e, checker);
            }
        }
        _ => {}
    }
}

/// Scan for transactional rollback annotations.
fn run_rollback_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = RollbackChecker::new();
    let mut scan_errors = Vec::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "rollback" || k == "savepoint" || k == "transactional")
            {
                found = true;
                scan_errors.extend(rollback_scan_expr(&clause.body, &mut checker));
            }
            if found && matches!(clause.kind, ClauseKind::Requires | ClauseKind::Ensures) {
                scan_errors.extend(rollback_scan_expr(&clause.body, &mut checker));
            }
        }
    }
    if !found {
        return Vec::new();
    }
    let mut errors = scan_errors;
    errors.extend(checker.check_resource_leak());
    errors.extend(checker.check_savepoint_nesting());
    errors
}

/// Scan an expression for rollback operations (savepoint, acquire, release, rollback).
/// Returns any immediate errors (e.g., rollback to unknown savepoint).
fn rollback_scan_expr(expr: &Expr, checker: &mut RollbackChecker) -> Vec<TypeError> {
    let mut scan_errors = Vec::new();
    if let Some((name, args)) = extract_call(expr) {
        match name {
            "savepoint" | "create_savepoint" => {
                let sp_name = args
                    .first()
                    .and_then(extract_ident)
                    .unwrap_or("default")
                    .to_string();
                checker.create_savepoint(sp_name);
            }
            "acquire" | "acquire_resource" | "lock" => {
                let res_name = args
                    .first()
                    .and_then(extract_ident)
                    .unwrap_or("resource")
                    .to_string();
                checker.acquire_resource(res_name);
            }
            "release" | "release_resource" | "unlock" => {
                let res_name = args.first().and_then(extract_ident).unwrap_or("resource");
                checker.release_resource(res_name);
            }
            "rollback" | "rollback_to" => {
                let sp_name = args.first().and_then(extract_ident).unwrap_or("default");
                if let Some(err) = checker.rollback_to(sp_name) {
                    scan_errors.push(err);
                }
            }
            _ => {}
        }
    }
    // Also check for identifier-based savepoint declarations
    if let Expr::Ident(name) = expr {
        checker.create_savepoint(name.clone());
    }
    // Scan sub-expressions in blocks/lists
    match expr {
        Expr::Block(exprs) | Expr::List(exprs) => {
            for e in exprs {
                scan_errors.extend(rollback_scan_expr(e, checker));
            }
        }
        _ => {}
    }
    scan_errors
}

/// Scan for monotonic state annotations and check update direction.
fn run_monotonic_state_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = MonotonicStateChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind {
                if k == "monotonic" || k == "monotone" || k == "increasing" {
                    found = true;
                    // Extract direction from call syntax: monotonic(name, direction, initial)
                    match &clause.body {
                        Expr::Call { func, args } => {
                            if let Expr::Ident(name) = func.as_ref() {
                                let direction = args
                                    .first()
                                    .and_then(extract_ident)
                                    .map(|d| match d {
                                        "strictly_increasing" => {
                                            MonotonicDirection::StrictlyIncreasing
                                        }
                                        "decreasing" => MonotonicDirection::Decreasing,
                                        _ => MonotonicDirection::Increasing,
                                    })
                                    .unwrap_or(MonotonicDirection::Increasing);
                                let initial =
                                    args.get(1).and_then(extract_int_literal).unwrap_or(0);
                                checker.declare(
                                    name.clone(),
                                    direction,
                                    initial,
                                    decl.span.clone(),
                                );
                            }
                        }
                        Expr::Ident(name) => {
                            checker.declare(
                                name.clone(),
                                MonotonicDirection::Increasing,
                                0,
                                decl.span.clone(),
                            );
                        }
                        _ => {}
                    }
                }
                // Check updates: update(name, value)
                if (k == "update" || k == "assign" || k == "set")
                    && let Some((name, args)) = extract_call(&clause.body)
                    && let Some(val) = args.first().and_then(extract_int_literal)
                    && let Some(err) = checker.update(name, val)
                {
                    return vec![err];
                }
                // Check resets
                if k == "reset"
                    && let Some(name) = extract_ident(&clause.body)
                    && let Some(err) = checker.check_reset(name)
                {
                    return vec![err];
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    // Check ensures clauses for monotonicity violations via identifier usage
    let mut errors = Vec::new();
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if clause.kind == ClauseKind::Ensures {
                let refs = collect_ident_references(&clause.body);
                for name in &refs {
                    if let Some(mut err) = checker.check_access(name) {
                        if let Some(val) = checker.current_value(name) {
                            err.message.push_str(&format!(" (current value: {val})"));
                        }
                        errors.push(err);
                    }
                }
            }
        }
    }
    errors
}

/// Scan for storage failure model annotations.
fn run_storage_failure_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = StorageFailureChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind {
                if k == "failure_mode" || k == "storage_failure" {
                    found = true;
                    if let Expr::Ident(name) = &clause.body {
                        let mode = match name.as_str() {
                            "partial_write" => FailureMode::PartialWrite,
                            "torn_page" => FailureMode::TornPage,
                            "bit_rot" => FailureMode::BitRot,
                            "disk_full" => FailureMode::DiskFull,
                            "io_timeout" => FailureMode::IoTimeout,
                            _ => continue,
                        };
                        checker.declare_failure_mode(mode);
                    }
                }
                if (k == "handles" || k == "handles_failure")
                    && let Expr::Ident(name) = &clause.body
                {
                    checker.mark_handled(name);
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    let mut errors = checker.check_unhandled();
    errors.extend(checker.check_critical_coverage());
    errors.extend(checker.check_spurious_handlers());
    errors
}

/// Scan for numerical precision annotations and check precision bounds.
fn run_numerical_precision_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = NumericalPrecisionChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "precision" || k == "numerical_precision" || k == "ulp_bound")
            {
                found = true;
                // Extract precision params from call syntax: precision(name, bits, ulp)
                match &clause.body {
                    Expr::Call { func, args } => {
                        if let Expr::Ident(name) = func.as_ref() {
                            let bits =
                                args.first().and_then(extract_int_literal).unwrap_or(64) as u32;
                            let ulp = args.get(1).and_then(extract_float_literal).unwrap_or(1.0);
                            checker.declare(name.clone(), bits, ulp, decl.span.clone());
                        }
                    }
                    Expr::Ident(name) => {
                        checker.declare(name.clone(), 64, 1.0, decl.span.clone());
                    }
                    _ => {
                        let kvs = extract_kv_pairs(&clause.body);
                        let name = kvs
                            .iter()
                            .find(|(k, _)| *k == "name" || *k == "var")
                            .and_then(|(_, v)| extract_ident(v))
                            .unwrap_or("unnamed")
                            .to_string();
                        let bits = kvs
                            .iter()
                            .find(|(k, _)| *k == "bits")
                            .and_then(|(_, v)| extract_int_literal(v))
                            .unwrap_or(64) as u32;
                        let ulp = kvs
                            .iter()
                            .find(|(k, _)| *k == "ulp")
                            .and_then(|(_, v)| extract_float_literal(v))
                            .unwrap_or(1.0);
                        checker.declare(name, bits, ulp, decl.span.clone());
                    }
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    // Check precision in ensures clauses for referenced variables
    let mut errors = Vec::new();
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if clause.kind == ClauseKind::Ensures {
                let refs = collect_ident_references(&clause.body);
                for name in &refs {
                    // Detect narrowing operations (e.g., 64-bit -> 32-bit)
                    if let Some(err) = checker.check_precision_loss(name, 32) {
                        // Only flag if there is a cast-like expression pattern
                        if clause_contains_cast(&clause.body, name) {
                            errors.push(err);
                        }
                    }
                    // Check ULP bound violations
                    if let Some(err) = checker.check_ulp_bound(name, 2.0)
                        && clause_contains_cast(&clause.body, name)
                    {
                        errors.push(err);
                    }
                    // Check catastrophic cancellation
                    if let Some(err) = checker.check_cancellation(name, 0.9999) {
                        errors.push(err);
                    }
                }
            }
        }
    }
    errors
}

/// Check if a clause body contains a cast-like expression for a variable.
fn clause_contains_cast(expr: &Expr, var_name: &str) -> bool {
    match expr {
        Expr::Cast { expr: inner, .. } => {
            if let Expr::Ident(name) = inner.as_ref() {
                name == var_name
            } else {
                clause_contains_cast(inner, var_name)
            }
        }
        Expr::Call { func, args } => {
            if let Expr::Ident(fn_name) = func.as_ref()
                && (fn_name == "as_f32" || fn_name == "narrow" || fn_name == "truncate")
                && args
                    .iter()
                    .any(|a| matches!(a, Expr::Ident(n) if n == var_name))
            {
                return true;
            }
            args.iter().any(|a| clause_contains_cast(a, var_name))
        }
        Expr::BinOp { lhs, rhs, .. } => {
            clause_contains_cast(lhs, var_name) || clause_contains_cast(rhs, var_name)
        }
        Expr::Paren(inner) => clause_contains_cast(inner, var_name),
        _ => false,
    }
}

/// Scan for precomputed table annotations.
fn run_precomputed_table_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = PrecomputedTableChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "precomputed_table" || k == "lookup_table" || k == "const_table")
            {
                found = true;
                // Extract table params: precomputed_table(name, size, generator)
                match &clause.body {
                    Expr::Call { func, args } => {
                        if let Expr::Ident(name) = func.as_ref() {
                            let size =
                                args.first().and_then(extract_int_literal).unwrap_or(256) as usize;
                            let gen_fn = args
                                .get(1)
                                .and_then(extract_ident)
                                .unwrap_or("")
                                .to_string();
                            checker.declare_table(name.clone(), size, gen_fn, decl.span.clone());
                        }
                    }
                    Expr::Ident(name) => {
                        checker.declare_table(name.clone(), 256, String::new(), decl.span.clone());
                    }
                    _ => {
                        let kvs = extract_kv_pairs(&clause.body);
                        let name = kvs
                            .iter()
                            .find(|(k, _)| *k == "name" || *k == "table")
                            .and_then(|(_, v)| extract_ident(v))
                            .unwrap_or("unnamed")
                            .to_string();
                        let size = kvs
                            .iter()
                            .find(|(k, _)| *k == "size" || *k == "entries")
                            .and_then(|(_, v)| extract_int_literal(v))
                            .unwrap_or(256) as usize;
                        let gen_fn = kvs
                            .iter()
                            .find(|(k, _)| *k == "generator" || *k == "gen")
                            .and_then(|(_, v)| extract_ident(v))
                            .unwrap_or("")
                            .to_string();
                        checker.declare_table(name, size, gen_fn, decl.span.clone());
                    }
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    // Mark entries as verified if verification clauses exist
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "verified_entries" || k == "table_verified")
                && let Some((name, args)) = extract_call(&clause.body)
            {
                let count = args.first().and_then(extract_int_literal).unwrap_or(0) as usize;
                checker.mark_entries_verified(name, count);
            }
        }
    }
    let mut errors = checker.check_coverage();
    errors.extend(checker.check_generator());
    errors.extend(checker.check_non_empty());
    errors
}

/// Scan for platform abstraction annotations.
fn run_platform_abstraction_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = PlatformAbstractionChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind {
                if k == "platform" || k == "target_platform" {
                    found = true;
                    if let Expr::Ident(name) = &clause.body {
                        checker.add_platform(name.clone());
                    }
                }
                if k == "abstraction" || k == "platform_abstraction" {
                    match &clause.body {
                        Expr::Call { func, args } => {
                            if let Expr::Ident(name) = func.as_ref() {
                                let platforms: Vec<String> = args
                                    .iter()
                                    .filter_map(|a| extract_ident(a).map(String::from))
                                    .collect();
                                checker.declare_abstraction(name.clone(), platforms);
                            }
                        }
                        Expr::Ident(name) => {
                            // Collect platforms declared so far as supported
                            let platforms = checker.known_platforms().to_vec();
                            checker.declare_abstraction(name.clone(), platforms);
                        }
                        _ => {
                            let kvs = extract_kv_pairs(&clause.body);
                            let name = kvs
                                .iter()
                                .find(|(k, _)| *k == "name" || *k == "abstraction")
                                .and_then(|(_, v)| extract_ident(v))
                                .unwrap_or("unnamed")
                                .to_string();
                            let platforms: Vec<String> = kvs
                                .iter()
                                .filter(|(k, _)| *k == "platform" || *k == "supports")
                                .filter_map(|(_, v)| extract_ident(v).map(String::from))
                                .collect();
                            checker.declare_abstraction(name, platforms);
                        }
                    }
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    let mut errors = checker.check_coverage();
    errors.extend(checker.check_unknown_platforms());
    // Check for direct platform use in clause bodies
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                let refs = collect_ident_references(&clause.body);
                for name in &refs {
                    if let Some(err) = checker.check_direct_platform_use(name) {
                        errors.push(err);
                    }
                }
            }
        }
    }
    errors
}

/// Scan for feature flag annotations.
fn run_feature_flag_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = FeatureFlagChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "feature_flag" || k == "feature" || k == "flag")
            {
                found = true;
                match &clause.body {
                    Expr::Call { func, args } => {
                        if let Expr::Ident(name) = func.as_ref() {
                            let enabled = args
                                .first()
                                .and_then(extract_ident)
                                .is_some_and(|v| v == "true" || v == "enabled" || v == "on");
                            let deps: Vec<String> = args
                                .iter()
                                .skip(1)
                                .filter_map(|a| extract_ident(a).map(String::from))
                                .collect();
                            checker.declare(name.clone(), enabled, deps);
                        }
                    }
                    Expr::Ident(name) => {
                        checker.declare(name.clone(), false, Vec::new());
                    }
                    _ => {
                        let kvs = extract_kv_pairs(&clause.body);
                        let name = kvs
                            .iter()
                            .find(|(k, _)| *k == "name" || *k == "flag")
                            .and_then(|(_, v)| extract_ident(v))
                            .unwrap_or("unnamed")
                            .to_string();
                        let enabled = kvs
                            .iter()
                            .find(|(k, _)| *k == "default" || *k == "enabled")
                            .and_then(|(_, v)| extract_ident(v))
                            .is_some_and(|v| v == "true" || v == "enabled" || v == "on");
                        let deps: Vec<String> = kvs
                            .iter()
                            .filter(|(k, _)| *k == "depends_on" || *k == "requires")
                            .filter_map(|(_, v)| extract_ident(v).map(String::from))
                            .collect();
                        checker.declare(name, enabled, deps);
                    }
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    // Mark flags as used and check for undeclared references in clause bodies
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            _ => continue,
        };
        for clause in clauses {
            if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                let refs = collect_ident_references(&clause.body);
                for name in &refs {
                    checker.mark_used(name);
                }
            }
            // Check for undeclared flag references
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "when_flag" || k == "if_feature")
                && let Expr::Ident(flag_name) = &clause.body
                && let Some(err) = checker.check_undeclared(flag_name)
            {
                return vec![err];
            }
        }
    }
    let mut errors = checker.check_unused();
    errors.extend(checker.check_conflicts());
    errors
}

/// Scan for resource limit annotations.
fn run_resource_limit_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = ResourceLimitChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "resource_limit" || k == "limit" || k == "quota")
            {
                found = true;
                // Extract limit: limit(name, max, unit)
                match &clause.body {
                    Expr::Call { func, args } => {
                        if let Expr::Ident(name) = func.as_ref() {
                            let max_val =
                                args.first()
                                    .and_then(extract_int_literal)
                                    .unwrap_or(i64::MAX) as u64;
                            let unit = args
                                .get(1)
                                .and_then(extract_ident)
                                .unwrap_or("units")
                                .to_string();
                            checker.declare_limit(name.clone(), max_val, unit);
                        }
                    }
                    Expr::Ident(name) => {
                        // Bare identifier without explicit max: flag as unbounded via check_unbounded
                        checker.declare_limit(name.clone(), u64::MAX, "units".into());
                    }
                    _ => {
                        let kvs = extract_kv_pairs(&clause.body);
                        let name = kvs
                            .iter()
                            .find(|(k, _)| *k == "name" || *k == "resource")
                            .and_then(|(_, v)| extract_ident(v))
                            .unwrap_or("unnamed")
                            .to_string();
                        let max_val = kvs
                            .iter()
                            .find(|(k, _)| *k == "max" || *k == "limit")
                            .and_then(|(_, v)| extract_int_literal(v))
                            .unwrap_or(i64::MAX) as u64;
                        let unit = kvs
                            .iter()
                            .find(|(k, _)| *k == "unit" || *k == "units")
                            .and_then(|(_, v)| extract_ident(v))
                            .unwrap_or("units")
                            .to_string();
                        checker.declare_limit(name, max_val, unit);
                    }
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    let mut errors = Vec::new();
    // Track resource usage and release from clause bodies
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind {
                if (k == "use_resource" || k == "consume")
                    && let Some((name, args)) = extract_call(&clause.body)
                {
                    let amount = args.first().and_then(extract_int_literal).unwrap_or(1) as u64;
                    checker.record_usage(name, amount);
                }
                if (k == "release_resource" || k == "free_resource")
                    && let Some((name, args)) = extract_call(&clause.body)
                {
                    let amount = args.first().and_then(extract_int_literal).unwrap_or(1) as u64;
                    checker.release_usage(name, amount);
                }
            }
            // Check for unbounded resource references in clause bodies
            if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                let refs = collect_ident_references(&clause.body);
                for name in &refs {
                    if let Some(err) = checker.check_unbounded(name) {
                        errors.push(err);
                    }
                }
            }
        }
    }
    errors.extend(checker.check_limits());
    errors.extend(checker.check_near_limit());
    errors
}

/// Scan for unsafe escape blocks and check proof obligations.
fn run_unsafe_escape_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = UnsafeEscapeChecker::new();
    let mut found = false;
    for decl in &source.decls {
        match &decl.node {
            Decl::FnDef(f) => {
                let mut obligations = Vec::new();
                for clause in &f.clauses {
                    if let ClauseKind::Other(ref k) = clause.kind
                        && (k == "obligation" || k == "proof_obligation" || k == "must_prove")
                    {
                        if let Expr::Ident(obl) = &clause.body {
                            obligations.push(obl.clone());
                        } else if let Some((_, args)) = extract_call(&clause.body) {
                            for arg in args {
                                if let Some(name) = extract_ident(arg) {
                                    obligations.push(name.to_string());
                                }
                            }
                        }
                    }
                }
                for clause in &f.clauses {
                    if let ClauseKind::Other(ref k) = clause.kind {
                        if k == "unsafe" || k == "unsafe_escape" || k == "trusted" {
                            found = true;
                            checker.declare_unsafe(
                                f.name.clone(),
                                obligations.clone(),
                                decl.span.clone(),
                            );
                        }
                        if k == "safety_proof" || k == "proof" {
                            checker.attach_proof(&f.name);
                        }
                    }
                }
            }
            Decl::Block {
                kind, name, body, ..
            } if *kind == BlockKind::UnsafeEscape => {
                found = true;
                let mut obligations = Vec::new();
                for clause in body {
                    if let ClauseKind::Other(ref k) = clause.kind
                        && (k == "obligation" || k == "proof_obligation" || k == "must_prove")
                    {
                        if let Expr::Ident(obl) = &clause.body {
                            obligations.push(obl.clone());
                        } else if let Some((_, args)) = extract_call(&clause.body) {
                            for arg in args {
                                if let Some(name) = extract_ident(arg) {
                                    obligations.push(name.to_string());
                                }
                            }
                        }
                    }
                }
                checker.declare_unsafe(name.clone(), obligations, decl.span.clone());
                for clause in body {
                    if let ClauseKind::Other(ref k) = clause.kind
                        && (k == "safety_proof" || k == "proof")
                    {
                        checker.attach_proof(name);
                    }
                }
            }
            _ => {}
        }
    }
    if !found {
        return Vec::new();
    }
    // Discharge obligations from proof clauses
    for decl in &source.decls {
        match &decl.node {
            Decl::FnDef(f) => {
                for clause in &f.clauses {
                    if let ClauseKind::Other(ref k) = clause.kind
                        && (k == "discharges" || k == "proves")
                        && let Expr::Ident(obligation) = &clause.body
                    {
                        checker.discharge_obligation(&f.name, obligation.clone());
                    }
                }
            }
            Decl::Block {
                kind, name, body, ..
            } if *kind == BlockKind::UnsafeEscape => {
                for clause in body {
                    if let ClauseKind::Other(ref k) = clause.kind
                        && (k == "discharges" || k == "proves")
                        && let Expr::Ident(obligation) = &clause.body
                    {
                        checker.discharge_obligation(name, obligation.clone());
                    }
                }
            }
            _ => {}
        }
    }
    let mut errors = checker.check_unproven();
    errors.extend(checker.check_obligations());
    errors.extend(checker.check_empty_obligations());
    errors
}

/// Scan for complexity bound annotations.
fn run_complexity_bound_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = ComplexityBoundChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::FnDef(f) => &f.clauses,
            Decl::Contract(c) => &c.clauses,
            _ => continue,
        };
        let name = match &decl.node {
            Decl::FnDef(f) => f.name.clone(),
            Decl::Contract(c) => c.name.clone(),
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "complexity" || k == "time_complexity" || k == "big_o")
            {
                found = true;
                if let Expr::Ident(class_name) = &clause.body {
                    let class = match class_name.as_str() {
                        "constant" | "O1" => ComplexityClass::Constant,
                        "logarithmic" | "O_log_n" => ComplexityClass::Logarithmic,
                        "linear" | "On" => ComplexityClass::Linear,
                        "nlogn" | "O_n_log_n" => ComplexityClass::NLogN,
                        "quadratic" | "On2" => ComplexityClass::Quadratic,
                        "cubic" | "On3" => ComplexityClass::Cubic,
                        "exponential" | "O2n" => ComplexityClass::Exponential,
                        _ => ComplexityClass::Linear,
                    };
                    checker.declare_bound(name.clone(), class, decl.span.clone());
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    // Record measured complexity from annotations
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::FnDef(f) => &f.clauses,
            Decl::Contract(c) => &c.clauses,
            _ => continue,
        };
        let name = match &decl.node {
            Decl::FnDef(f) => f.name.as_str(),
            Decl::Contract(c) => c.name.as_str(),
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "measured_complexity" || k == "actual_complexity")
                && let Expr::Ident(class_name) = &clause.body
            {
                let class = match class_name.as_str() {
                    "constant" | "O1" => ComplexityClass::Constant,
                    "logarithmic" | "O_log_n" => ComplexityClass::Logarithmic,
                    "linear" | "On" => ComplexityClass::Linear,
                    "nlogn" | "O_n_log_n" => ComplexityClass::NLogN,
                    "quadratic" | "On2" => ComplexityClass::Quadratic,
                    "cubic" | "On3" => ComplexityClass::Cubic,
                    "exponential" | "O2n" => ComplexityClass::Exponential,
                    _ => ComplexityClass::Linear,
                };
                checker.record_measured(name, class);
            }
        }
    }
    let mut errors = checker.check_bounds();
    errors.extend(checker.check_unverified());
    errors.extend(checker.check_expensive());
    errors
}

/// Scan for behavioral equivalence annotations.
fn run_behavioral_equivalence_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = BehavioralEquivalenceChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "equivalent" || k == "behavioral_equiv" || k == "equiv")
            {
                found = true;
                if let Expr::BinOp { lhs, rhs, .. } = &clause.body
                    && let (Expr::Ident(a), Expr::Ident(b)) = (lhs.as_ref(), rhs.as_ref())
                {
                    checker.declare(
                        format!("{a}_equiv_{b}"),
                        a.clone(),
                        b.clone(),
                        String::new(),
                        decl.span.clone(),
                    );
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    // Mark equivalences as verified if proof clauses exist
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "verified_equiv" || k == "equiv_proved")
                && let Expr::Ident(name) = &clause.body
            {
                checker.mark_verified(name);
            }
        }
    }
    let mut errors = checker.check_unverified();
    errors.extend(checker.check_self_equivalence());
    errors.extend(checker.check_contract_ref());
    errors
}

/// Scan for multi-pass refinement annotations.
fn run_multi_pass_refinement_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = MultiPassRefinementChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "refinement_pass" || k == "multi_pass" || k == "refine")
            {
                found = true;
                // Extract pass params: refine(name, from_level, to_level, order)
                match &clause.body {
                    Expr::Call { func, args } => {
                        if let Expr::Ident(name) = func.as_ref() {
                            let from = args
                                .first()
                                .and_then(extract_ident)
                                .unwrap_or("abstract")
                                .to_string();
                            let to = args
                                .get(1)
                                .and_then(extract_ident)
                                .unwrap_or("concrete")
                                .to_string();
                            let order =
                                args.get(2).and_then(extract_int_literal).unwrap_or(1) as usize;
                            checker.add_pass(name.clone(), from, to, order, decl.span.clone());
                        }
                    }
                    Expr::Ident(name) => {
                        checker.add_pass(
                            name.clone(),
                            "abstract".into(),
                            "concrete".into(),
                            1,
                            decl.span.clone(),
                        );
                    }
                    _ => {
                        let kvs = extract_kv_pairs(&clause.body);
                        let name = kvs
                            .iter()
                            .find(|(k, _)| *k == "name" || *k == "pass")
                            .and_then(|(_, v)| extract_ident(v))
                            .unwrap_or("unnamed")
                            .to_string();
                        let from = kvs
                            .iter()
                            .find(|(k, _)| *k == "from" || *k == "source")
                            .and_then(|(_, v)| extract_ident(v))
                            .unwrap_or("abstract")
                            .to_string();
                        let to = kvs
                            .iter()
                            .find(|(k, _)| *k == "to" || *k == "target")
                            .and_then(|(_, v)| extract_ident(v))
                            .unwrap_or("concrete")
                            .to_string();
                        let order = kvs
                            .iter()
                            .find(|(k, _)| *k == "order")
                            .and_then(|(_, v)| extract_int_literal(v))
                            .unwrap_or(1) as usize;
                        checker.add_pass(name, from, to, order, decl.span.clone());
                    }
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    // Discharge refinement obligations from proof annotations
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "discharge_pass" || k == "pass_proved")
                && let Some((name, args)) = extract_call(&clause.body)
            {
                let count = args.first().and_then(extract_int_literal).unwrap_or(1) as usize;
                checker.discharge(name, count);
            }
        }
    }
    let mut errors = checker.check_complete();
    errors.extend(checker.check_chain());
    errors.extend(checker.check_non_trivial());
    errors
}

/// Scan for incremental contract version annotations.
fn run_incremental_contract_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = IncrementalContractChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        // Count requires/ensures clauses for this declaration
        let requires_count = clauses
            .iter()
            .filter(|c| matches!(c.kind, ClauseKind::Requires))
            .count();
        let ensures_count = clauses
            .iter()
            .filter(|c| matches!(c.kind, ClauseKind::Ensures))
            .count();
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "version" || k == "incremental" || k == "contract_version")
            {
                found = true;
                // Extract version: version(name, major, minor, patch)
                match &clause.body {
                    Expr::Call { func, args } => {
                        if let Expr::Ident(name) = func.as_ref() {
                            let major =
                                args.first().and_then(extract_int_literal).unwrap_or(1) as u32;
                            let minor =
                                args.get(1).and_then(extract_int_literal).unwrap_or(0) as u32;
                            let patch =
                                args.get(2).and_then(extract_int_literal).unwrap_or(0) as u32;
                            let version = major * 10000 + minor * 100 + patch;
                            checker.add_version(
                                name.clone(),
                                version,
                                requires_count,
                                ensures_count,
                            );
                        }
                    }
                    Expr::Ident(name) => {
                        checker.add_version(name.clone(), 10000, requires_count, ensures_count);
                    }
                    _ => {
                        let kvs = extract_kv_pairs(&clause.body);
                        let name = kvs
                            .iter()
                            .find(|(k, _)| *k == "name" || *k == "contract")
                            .and_then(|(_, v)| extract_ident(v))
                            .unwrap_or("unnamed")
                            .to_string();
                        let major = kvs
                            .iter()
                            .find(|(k, _)| *k == "major")
                            .and_then(|(_, v)| extract_int_literal(v))
                            .unwrap_or(1) as u32;
                        let minor = kvs
                            .iter()
                            .find(|(k, _)| *k == "minor")
                            .and_then(|(_, v)| extract_int_literal(v))
                            .unwrap_or(0) as u32;
                        let patch = kvs
                            .iter()
                            .find(|(k, _)| *k == "patch")
                            .and_then(|(_, v)| extract_int_literal(v))
                            .unwrap_or(0) as u32;
                        let version = major * 10000 + minor * 100 + patch;
                        checker.add_version(name, version, requires_count, ensures_count);
                    }
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    let mut errors = checker.check_precondition_weakening();
    errors.extend(checker.check_postcondition_strengthening());
    errors.extend(checker.check_version_continuity());
    errors
}

/// Scan for scoped invariant suspension annotations.
fn run_scoped_invariant_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = ScopedInvariantChecker::new();
    let mut errors = Vec::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind {
                if k == "suspend_invariant" || k == "scoped_invariant" {
                    found = true;
                    if let Expr::Ident(name) = &clause.body {
                        checker.declare_invariant(name.clone());
                        if let Some(err) = checker.suspend(name) {
                            errors.push(err);
                        }
                    }
                }
                if (k == "restore_invariant" || k == "restore")
                    && let Expr::Ident(name) = &clause.body
                    && let Some(err) = checker.restore(name)
                {
                    errors.push(err);
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    // Check individual invariant suspension status in clause bodies
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                let refs = collect_ident_references(&clause.body);
                for name in &refs {
                    if checker.is_suspended(name) {
                        errors.push(TypeError {
                            code: "A52001".into(),
                            message: format!(
                                "invariant `{name}` is suspended in active clause context"
                            ),
                            span: decl.span.clone(),
                            secondary: None,
                        });
                    }
                }
            }
        }
    }
    errors.extend(checker.check_all_restored());
    errors
}

/// Scan for contract composition (extends) and validate.
fn run_contract_composition_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = ContractCompositionChecker::new();
    let mut found = false;
    for decl in &source.decls {
        if let Decl::Contract(c) = &decl.node {
            let extends: Vec<String> = c
                .clauses
                .iter()
                .filter(|cl| {
                    matches!(&cl.kind, ClauseKind::Other(k) if k == "extends" || k == "inherits")
                })
                .filter_map(|cl| {
                    if let Expr::Ident(name) = &cl.body {
                        Some(name.clone())
                    } else {
                        None
                    }
                })
                .collect();
            if !extends.is_empty() {
                found = true;
            }
            checker.declare(c.name.clone(), extends, c.clauses.len());
        }
    }
    if !found {
        return Vec::new();
    }
    let mut errors = checker.check_extends();
    errors.extend(checker.check_circular());
    errors.extend(checker.check_diamond());
    errors.extend(checker.check_empty_contracts());
    errors
}

/// Scan for contract library packaging declarations.
fn run_contract_library_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = ContractLibraryChecker::new();
    let mut found = false;
    for decl in &source.decls {
        match &decl.node {
            Decl::Block {
                kind, name, body, ..
            } if *kind == BlockKind::Library => {
                found = true;
                checker.declare_library(name.clone(), "0.1.0".into());
                for clause in body {
                    if let ClauseKind::Other(ref k) = clause.kind {
                        if (k == "export" || k == "exports")
                            && let Expr::Ident(contract_name) = &clause.body
                        {
                            checker.add_export(name, contract_name.clone());
                        }
                        if (k == "depends" || k == "dependency")
                            && let Expr::Ident(dep_name) = &clause.body
                        {
                            checker.add_dependency(
                                name,
                                LibraryDep {
                                    name: dep_name.clone(),
                                    version_req: "*".into(),
                                },
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if !found {
        return Vec::new();
    }
    let mut errors = checker.check_empty_exports();
    errors.extend(checker.check_circular_deps());
    errors.extend(checker.check_duplicates());
    errors.extend(checker.check_version_compat());
    errors
}

#[cfg(test)]
mod type_from_expr_tests {
    use super::*;
    use assura_parser::ast::TypeExpr;

    #[test]
    fn named_builtin() {
        assert_eq!(type_from_expr(&TypeExpr::Named("Int".into())), Type::Int);
        assert_eq!(type_from_expr(&TypeExpr::Named("Bool".into())), Type::Bool);
    }

    #[test]
    fn named_user_defined() {
        assert_eq!(
            type_from_expr(&TypeExpr::Named("MyType".into())),
            Type::Named("MyType".into())
        );
    }

    #[test]
    fn generic_list() {
        let te = TypeExpr::Generic("List".into(), vec![TypeExpr::Named("Int".into())]);
        assert_eq!(type_from_expr(&te), Type::List(Box::new(Type::Int)));
    }

    #[test]
    fn generic_map() {
        let te = TypeExpr::Generic(
            "Map".into(),
            vec![
                TypeExpr::Named("String".into()),
                TypeExpr::Named("Int".into()),
            ],
        );
        assert_eq!(
            type_from_expr(&te),
            Type::Map(Box::new(Type::String), Box::new(Type::Int))
        );
    }

    #[test]
    fn unit_and_tuple() {
        assert_eq!(type_from_expr(&TypeExpr::Unit), Type::Unit);
        let te = TypeExpr::Tuple(vec![
            TypeExpr::Named("Int".into()),
            TypeExpr::Named("Bool".into()),
        ]);
        assert_eq!(
            type_from_expr(&te),
            Type::Tuple(vec![Type::Int, Type::Bool])
        );
    }

    #[test]
    fn fn_type() {
        let te = TypeExpr::Fn {
            params: vec![TypeExpr::Named("Int".into())],
            ret: Box::new(TypeExpr::Named("Bool".into())),
        };
        assert_eq!(
            type_from_expr(&te),
            Type::Fn {
                params: vec![Type::Int],
                ret: Box::new(Type::Bool),
            }
        );
    }

    #[test]
    fn resolve_prefers_parsed() {
        let te = TypeExpr::Named("Int".into());
        let tokens = vec!["Bool".to_string()];
        assert_eq!(resolve_type(Some(&te), &tokens), Type::Int);
    }

    #[test]
    fn resolve_falls_back() {
        let tokens = vec!["Bool".to_string()];
        assert_eq!(resolve_type(None, &tokens), Type::Bool);
    }

    #[test]
    fn generic_instantiation_correct_arity() {
        let source = assura_parser::ast::SourceFile {
            project: None,
            module: None,
            imports: vec![],
            decls: vec![],
        };
        let span = 0..10;
        // List<Int> has 1 arg, expected 1
        assert!(check_generic_instantiation("List", &[Type::Int], &span, &source).is_ok());
        // Map<String, Int> has 2 args, expected 2
        assert!(
            check_generic_instantiation("Map", &[Type::String, Type::Int], &span, &source).is_ok()
        );
    }

    #[test]
    fn generic_instantiation_wrong_arity() {
        let source = assura_parser::ast::SourceFile {
            project: None,
            module: None,
            imports: vec![],
            decls: vec![],
        };
        let span = 0..10;
        // List<Int, Bool> has 2 args, expected 1
        let err = check_generic_instantiation("List", &[Type::Int, Type::Bool], &span, &source);
        assert!(err.is_err());
        let e = err.unwrap_err();
        assert_eq!(e.code, "A03002");
        assert!(e.message.contains("expected 1, found 2"));
    }

    #[test]
    fn generic_instantiation_user_defined() {
        use assura_parser::ast::{Decl, Spanned, TypeBody, TypeDef};
        let source = assura_parser::ast::SourceFile {
            project: None,
            module: None,
            imports: vec![],
            decls: vec![Spanned {
                node: Decl::TypeDef(TypeDef {
                    name: "Pair".into(),
                    type_params: vec!["A".into(), "B".into()],
                    body: TypeBody::Empty,
                }),
                span: 0..20,
            }],
        };
        let span = 0..10;
        // Pair<Int> has 1 arg, expected 2
        let err = check_generic_instantiation("Pair", &[Type::Int], &span, &source);
        assert!(err.is_err());
        assert!(err.unwrap_err().message.contains("expected 2, found 1"));
        // Pair<Int, Bool> has 2 args, expected 2
        assert!(
            check_generic_instantiation("Pair", &[Type::Int, Type::Bool], &span, &source).is_ok()
        );
    }

    #[test]
    fn run_generic_checks_catches_wrong_arity() {
        use assura_parser::ast::*;
        let source = SourceFile {
            project: None,
            module: None,
            imports: vec![],
            decls: vec![Spanned {
                node: Decl::FnDef(FnDef {
                    name: "bad_fn".into(),
                    is_ghost: false,
                    is_lemma: false,
                    params: vec![Param {
                        name: "x".into(),
                        ty: vec![
                            "List".into(),
                            "<".into(),
                            "Int".into(),
                            ",".into(),
                            "Bool".into(),
                            ">".into(),
                        ],
                        parsed_type: Some(TypeExpr::Generic(
                            "List".into(),
                            vec![
                                TypeExpr::Named("Int".into()),
                                TypeExpr::Named("Bool".into()),
                            ],
                        )),
                    }],
                    return_ty: vec![],
                    return_type_expr: None,
                    clauses: vec![],
                }),
                span: 0..50,
            }],
        };
        let errors = super::run_generic_instantiation_checks(&source);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A03002");
    }
}

#[cfg(test)]
mod type_from_hir_type_tests {
    use super::*;
    use assura_hir::HirType;

    #[test]
    fn hir_named_builtin() {
        assert_eq!(type_from_hir_type(&HirType::Named("Int".into())), Type::Int);
        assert_eq!(
            type_from_hir_type(&HirType::Named("Bool".into())),
            Type::Bool
        );
    }

    #[test]
    fn hir_named_user_defined() {
        assert_eq!(
            type_from_hir_type(&HirType::Named("MyType".into())),
            Type::Named("MyType".into())
        );
    }

    #[test]
    fn hir_generic_list() {
        let ht = HirType::Generic("List".into(), vec![HirType::Named("Int".into())]);
        assert_eq!(type_from_hir_type(&ht), Type::List(Box::new(Type::Int)));
    }

    #[test]
    fn hir_generic_map() {
        let ht = HirType::Generic(
            "Map".into(),
            vec![
                HirType::Named("String".into()),
                HirType::Named("Int".into()),
            ],
        );
        assert_eq!(
            type_from_hir_type(&ht),
            Type::Map(Box::new(Type::String), Box::new(Type::Int))
        );
    }

    #[test]
    fn hir_unit_and_tuple() {
        assert_eq!(type_from_hir_type(&HirType::Unit), Type::Unit);
        let ht = HirType::Tuple(vec![
            HirType::Named("Int".into()),
            HirType::Named("Bool".into()),
        ]);
        assert_eq!(
            type_from_hir_type(&ht),
            Type::Tuple(vec![Type::Int, Type::Bool])
        );
    }

    #[test]
    fn hir_fn_type() {
        let ht = HirType::Fn {
            params: vec![HirType::Named("Int".into())],
            ret: Box::new(HirType::Named("Bool".into())),
        };
        assert_eq!(
            type_from_hir_type(&ht),
            Type::Fn {
                params: vec![Type::Int],
                ret: Box::new(Type::Bool),
            }
        );
    }

    #[test]
    fn hir_refined() {
        let ht = HirType::Refined {
            base: Box::new(HirType::Named("Int".into())),
            predicate: "x > 0".into(),
        };
        assert_eq!(
            type_from_hir_type(&ht),
            Type::Refined {
                base: Box::new(Type::Int),
                predicate: "x > 0".into(),
            }
        );
    }

    #[test]
    fn hir_unresolved_falls_back() {
        let ht = HirType::Unresolved(vec!["Float".into()]);
        assert_eq!(type_from_hir_type(&ht), Type::Float);
    }
}

// ---------------------------------------------------------------------------
// G008: Codec registry validation (FMT.4)
// ---------------------------------------------------------------------------

/// Check codec registry declarations for:
/// - A52001: Overlapping magic byte patterns between codecs
/// - A52002: Empty decoder function name
fn run_codec_registry_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    use assura_parser::ast::MagicPattern;
    let mut errors = Vec::new();

    for decl in &source.decls {
        let cr = match &decl.node {
            Decl::CodecRegistry(cr) => cr,
            _ => continue,
        };

        // A52001: Check for overlapping magic byte prefixes
        let byte_patterns: Vec<(usize, &[u8])> = cr
            .codecs
            .iter()
            .enumerate()
            .filter_map(|(i, c)| match &c.magic {
                MagicPattern::Bytes { bytes, .. } if !bytes.is_empty() => {
                    Some((i, bytes.as_slice()))
                }
                _ => None,
            })
            .collect();

        for (i, (idx_a, bytes_a)) in byte_patterns.iter().enumerate() {
            for (idx_b, bytes_b) in byte_patterns.iter().skip(i + 1) {
                let min_len = bytes_a.len().min(bytes_b.len());
                if bytes_a[..min_len] == bytes_b[..min_len] {
                    errors.push(TypeError {
                        code: "A52001".into(),
                        message: format!(
                            "overlapping magic byte patterns in codec registry `{}`: \
                             codec `{}` and codec `{}` share a common prefix",
                            cr.name, cr.codecs[*idx_a].name, cr.codecs[*idx_b].name,
                        ),
                        span: decl.span.clone(),
                        secondary: None,
                    });
                }
            }
        }

        // A52002: Check for empty decoder names
        for codec in &cr.codecs {
            if codec.decoder.is_empty() {
                errors.push(TypeError {
                    code: "A52002".into(),
                    message: format!(
                        "codec `{}` in registry `{}` has no decoder function",
                        codec.name, cr.name,
                    ),
                    span: decl.span.clone(),
                    secondary: None,
                });
            }
        }
    }

    errors
}

#[cfg(test)]
mod tests;
