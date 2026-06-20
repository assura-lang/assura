//! Core type definitions for the Assura type checker.
//!
//! Contains the Type enum, TypeEnv, TypeError, TypedFile,
//! and the Display implementation for Type.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use assura_resolve::ResolvedFile;

use crate::checkers::PendingDecreaseCheck;

// ---- Domain-checker default constants ----
// Typed as `i64` to match `extract_int_literal` return type.

/// Default circular buffer / allocator capacity (bytes).
pub(crate) const DEFAULT_BUFFER_CAPACITY: i64 = 256;
/// Default temporal deadline (milliseconds).
pub(crate) const DEFAULT_DEADLINE_MS: i64 = 1000;
/// Default bit-level container width (bits).
pub(crate) const DEFAULT_BIT_CONTAINER_BITS: i64 = 64;
/// Default checksum / region size (bytes).
pub(crate) const DEFAULT_REGION_SIZE: i64 = 1024;
/// Default page-cache page size (bytes).
pub(crate) const DEFAULT_PAGE_SIZE: i64 = 1024;
/// Default feature-flag maximum count.
pub(crate) const DEFAULT_FEATURE_MAX: i64 = 256;
/// Default hash output length (bytes).
pub(crate) const DEFAULT_HASH_BITS: i64 = 32;

// ---- Numeric precision defaults ----

/// Default ULP (Unit in the Last Place) tolerance for numerical precision checks.
pub(crate) const DEFAULT_ULP_TOLERANCE: f64 = 1.0;

// ---- Parameter extraction defaults ----
// These represent "if the user didn't specify, use zero/one as the identity."

/// Default integer for absent clause arguments (zero value).
pub(crate) const DEFAULT_PARAM_ZERO: i64 = 0;
/// Default integer for absent clause arguments (unit value).
pub(crate) const DEFAULT_PARAM_ONE: i64 = 1;

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
    Named(String),

    // --- Generic type parameter ---
    TypeParam(String),

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
        predicate: String,
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
    pub bindings: HashMap<String, Type>,
    /// Maps struct type name -> { field_name -> field_type }.
    pub struct_fields: HashMap<String, Vec<(String, Type)>>,
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
    pub fn insert(&mut self, name: String, ty: Type) -> Option<Type> {
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
    pub message: String,
    /// Source location where the error was detected.
    pub span: Range<usize>,
    /// Optional secondary span with label (e.g., "expected type declared here").
    pub secondary: Option<(Range<usize>, String)>,
}

impl TypeError {
    /// Enrich the error message with additional context while preserving all other fields.
    pub fn with_context(self, context: &str) -> Self {
        Self {
            message: format!("{} ({context})", self.message),
            ..self
        }
    }
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
    /// Generated tests from contracts (TEST.1). Populated by the type
    /// checking pipeline when contracts have testable constraints.
    pub generated_tests: Vec<crate::GeneratedTest>,
}

// ---------------------------------------------------------------------------
// Built-in type mapping
// ---------------------------------------------------------------------------

/// Map a built-in type name to its `Type` representation.
pub(crate) fn builtin_type(name: &str) -> Option<Type> {
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
