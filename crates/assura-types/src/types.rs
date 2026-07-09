//! Core type definitions for the Assura type checker.
//!
//! Contains the Type enum, TypeEnv, TypeError, TypedFile,
//! and the Display implementation for Type.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use assura_parser::ast::{Expr, SpExpr, Spanned, expr_to_string};
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

/// Represents all Assura types in the type checker.
///
/// # Indeterminate types
///
/// Two variants represent "we don't have a concrete type":
/// - [`Unknown`](Type::Unknown): genuinely unknown (unresolved reference, missing type args)
/// - [`Error`](Type::Error): error already reported upstream; suppresses cascading diagnostics
///
/// Always use [`is_indeterminate()`](Type::is_indeterminate) instead of matching
/// `Type::Unknown` directly, to avoid missing `Error` and producing cascade false positives.
///
/// # Numeric types
///
/// `Int`, `Nat`, `Float`, and fixed-width variants (`U8`..`I64`, `F32`, `F64`) are all
/// considered numeric. Use `is_numeric()` to test.
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
        /// Parsed predicate expression (structural AST node).
        predicate: Box<SpExpr>,
        /// The variable bound by the refinement (e.g., "x" in `{x: Int | x > 0}`).
        bound_var: String,
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

    /// Construct a refined type from string-form predicate (convenience).
    ///
    /// The predicate text is stored as `Expr::Raw` tokens for backward
    /// compatibility. Use the constructor directly with a parsed `Expr`
    /// for structural analysis.
    pub fn refined_from_str(base: Type, bound_var: &str, predicate_text: &str) -> Self {
        let tokens: Vec<String> = if predicate_text.is_empty() {
            vec![]
        } else {
            predicate_text
                .split_whitespace()
                .map(String::from)
                .collect()
        };
        Type::Refined {
            base: Box::new(base),
            predicate: Box::new(Spanned::no_span(Expr::Raw(tokens))),
            bound_var: bound_var.to_string(),
        }
    }

    /// Get the predicate as a display string.
    pub fn predicate_str(&self) -> Option<String> {
        if let Type::Refined { predicate, .. } = self {
            let s = expr_to_string(predicate);
            if s.is_empty() { None } else { Some(s) }
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Type environment
// ---------------------------------------------------------------------------

/// Maps names to their types. This is the typing context built during
/// type checking.
#[derive(Debug, Clone, Default)]
pub struct TypeEnv {
    /// Scope stack: the last element is the innermost (current) scope.
    /// There is always at least one scope (the global scope).
    scopes: Vec<Scope>,
    /// Maps struct type name -> { field_name -> field_type }.
    /// Struct field definitions are always global (not scope-dependent).
    pub struct_fields: HashMap<String, Vec<(String, Type)>>,
}

/// A single scope level in the type environment.
#[derive(Debug, Clone, Default)]
struct Scope {
    bindings: HashMap<String, Type>,
}

impl TypeEnv {
    /// Create an empty type environment with one global scope.
    pub fn new() -> Self {
        Self {
            scopes: vec![Scope::default()],
            struct_fields: HashMap::new(),
        }
    }

    /// Push a new (empty) scope. Bindings inserted after this call
    /// shadow outer names and are removed when `pop_scope` is called.
    pub fn push_scope(&mut self) {
        self.scopes.push(Scope::default());
    }

    /// Pop the innermost scope, removing all bindings introduced in it.
    ///
    /// # Panics
    /// Panics if only the global scope remains (you cannot pop the root).
    pub fn pop_scope(&mut self) {
        assert!(
            self.scopes.len() > 1,
            "cannot pop the global scope from TypeEnv"
        );
        self.scopes.pop();
    }

    /// Current nesting depth (0 = global scope only).
    pub fn depth(&self) -> usize {
        self.scopes.len() - 1
    }

    /// Insert a binding into the *current* (innermost) scope.
    /// Returns the previous type if the name was already bound
    /// in this same scope.
    pub fn insert(&mut self, name: String, ty: Type) -> Option<Type> {
        self.scopes
            .last_mut()
            .expect("TypeEnv must have at least one scope")
            .bindings
            .insert(name, ty)
    }

    /// Look up a name, searching from the innermost scope outward.
    pub fn lookup(&self, name: &str) -> Option<&Type> {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.bindings.get(name) {
                return Some(ty);
            }
        }
        None
    }

    /// Look up a field type on a struct type.
    pub(crate) fn lookup_field(&self, struct_name: &str, field_name: &str) -> Option<&Type> {
        self.struct_fields
            .get(struct_name)
            .and_then(|fields| fields.iter().find(|(n, _)| n == field_name).map(|(_, t)| t))
    }

    /// Total number of bindings across all scopes.
    pub fn len(&self) -> usize {
        self.scopes.iter().map(|s| s.bindings.len()).sum()
    }

    /// Returns true if no bindings exist in any scope.
    pub fn is_empty(&self) -> bool {
        self.scopes.iter().all(|s| s.bindings.is_empty())
    }

    /// Iterate over all bindings from outermost to innermost scope.
    /// If a name appears in multiple scopes, only the innermost (shadowing)
    /// binding is yielded.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Type)> {
        let mut seen = HashMap::new();
        for scope in &self.scopes {
            for (name, ty) in &scope.bindings {
                seen.insert(name.as_str(), ty);
            }
        }
        seen.into_iter()
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
    /// Optional fix suggestion (e.g., "add an explicit type annotation").
    /// When `None`, the `From<TypeError> for Diagnostic` impl falls back to
    /// the error catalog's `fix` text for this error code (if any).
    pub suggestion: Option<String>,
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
        let mut d = assura_diagnostics::Diagnostic::error(e.code.clone(), e.message, e.span);
        if let Some((span, label)) = e.secondary {
            d.secondary.push(assura_diagnostics::SecondaryLabel {
                span,
                message: label,
            });
        }
        // Use the explicit suggestion if provided; otherwise fall back to
        // the error catalog's `fix` text for this error code.
        let suggestion_text = e.suggestion.or_else(|| {
            assura_diagnostics::explain(e.code.as_str()).map(|info| info.fix.to_string())
        });
        if let Some(text) = suggestion_text {
            let span = d.primary.clone();
            d = d.with_suggestion(text, span, "");
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
    /// Non-fatal warnings from type checking (e.g., unconstrained output
    /// references in ensures clauses, feature_max in verification clauses).
    pub warnings: Vec<TypeError>,
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
            Type::Refined {
                base,
                predicate,
                bound_var,
            } => {
                let pred_str = expr_to_string(predicate);
                if pred_str.is_empty() {
                    write!(f, "{base}")
                } else {
                    write!(f, "{{ {bound_var} : {base} | {pred_str} }}")
                }
            }
            Type::Unknown => write!(f, "Unknown"),
            Type::Error => write!(f, "<error>"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- is_indeterminate ----

    #[test]
    fn is_indeterminate_unknown() {
        assert!(Type::Unknown.is_indeterminate());
    }

    #[test]
    fn is_indeterminate_error() {
        assert!(Type::Error.is_indeterminate());
    }

    #[test]
    fn is_indeterminate_concrete_types_return_false() {
        let concrete = [
            Type::Int,
            Type::Nat,
            Type::Float,
            Type::Bool,
            Type::String,
            Type::Bytes,
            Type::Unit,
            Type::Never,
            Type::U8,
            Type::U16,
            Type::U32,
            Type::U64,
            Type::I8,
            Type::I16,
            Type::I32,
            Type::I64,
            Type::F32,
            Type::F64,
            Type::List(Box::new(Type::Int)),
            Type::Map(Box::new(Type::String), Box::new(Type::Int)),
            Type::Set(Box::new(Type::Nat)),
            Type::Option(Box::new(Type::Bool)),
            Type::Result(Box::new(Type::Int), Box::new(Type::String)),
            Type::Sequence(Box::new(Type::Int)),
            Type::Named("Foo".into()),
            Type::TypeParam("T".into()),
            Type::Fn {
                params: vec![Type::Int],
                ret: Box::new(Type::Bool),
            },
            Type::Tuple(vec![Type::Int, Type::Bool]),
            Type::refined_from_str(Type::Int, "x", "x > 0"),
        ];
        for ty in &concrete {
            assert!(!ty.is_indeterminate(), "{ty} should not be indeterminate");
        }
    }

    // ---- Display formatting ----

    #[test]
    fn display_base_types() {
        assert_eq!(Type::Int.to_string(), "Int");
        assert_eq!(Type::Nat.to_string(), "Nat");
        assert_eq!(Type::Float.to_string(), "Float");
        assert_eq!(Type::Bool.to_string(), "Bool");
        assert_eq!(Type::String.to_string(), "String");
        assert_eq!(Type::Bytes.to_string(), "Bytes");
        assert_eq!(Type::Unit.to_string(), "Unit");
        assert_eq!(Type::Never.to_string(), "Never");
    }

    #[test]
    fn display_fixed_width_integers() {
        assert_eq!(Type::U8.to_string(), "U8");
        assert_eq!(Type::U64.to_string(), "U64");
        assert_eq!(Type::I32.to_string(), "I32");
        assert_eq!(Type::F64.to_string(), "F64");
    }

    #[test]
    fn display_generic_containers() {
        assert_eq!(Type::List(Box::new(Type::Int)).to_string(), "List<Int>");
        assert_eq!(
            Type::Map(Box::new(Type::String), Box::new(Type::Nat)).to_string(),
            "Map<String, Nat>"
        );
        assert_eq!(Type::Set(Box::new(Type::Bool)).to_string(), "Set<Bool>");
        assert_eq!(
            Type::Option(Box::new(Type::Float)).to_string(),
            "Option<Float>"
        );
        assert_eq!(
            Type::Result(Box::new(Type::Int), Box::new(Type::String)).to_string(),
            "Result<Int, String>"
        );
        assert_eq!(
            Type::Sequence(Box::new(Type::Bytes)).to_string(),
            "Sequence<Bytes>"
        );
    }

    #[test]
    fn display_fn_type() {
        let ty = Type::Fn {
            params: vec![Type::Int, Type::Bool],
            ret: Box::new(Type::String),
        };
        assert_eq!(ty.to_string(), "fn(Int, Bool) -> String");
    }

    #[test]
    fn display_fn_no_params() {
        let ty = Type::Fn {
            params: vec![],
            ret: Box::new(Type::Unit),
        };
        assert_eq!(ty.to_string(), "fn() -> Unit");
    }

    #[test]
    fn display_tuple() {
        let ty = Type::Tuple(vec![Type::Int, Type::Bool, Type::String]);
        assert_eq!(ty.to_string(), "(Int, Bool, String)");
    }

    #[test]
    fn display_refined_with_predicate() {
        let ty = Type::refined_from_str(Type::Int, "x", "x > 0");
        assert_eq!(ty.to_string(), "{ x : Int | x > 0 }");
    }

    #[test]
    fn display_refined_empty_predicate() {
        let ty = Type::refined_from_str(Type::Nat, "x", "");
        // Empty predicate just displays the base type
        assert_eq!(ty.to_string(), "Nat");
    }

    #[test]
    fn display_unknown_and_error() {
        assert_eq!(Type::Unknown.to_string(), "Unknown");
        assert_eq!(Type::Error.to_string(), "<error>");
    }

    #[test]
    fn display_named_and_type_param() {
        assert_eq!(Type::Named("MyStruct".into()).to_string(), "MyStruct");
        assert_eq!(Type::TypeParam("T".into()).to_string(), "T");
    }

    #[test]
    fn display_nested_generics() {
        // List<Option<Int>>
        let ty = Type::List(Box::new(Type::Option(Box::new(Type::Int))));
        assert_eq!(ty.to_string(), "List<Option<Int>>");
    }

    // ---- builtin_type ----

    #[test]
    fn builtin_type_base_types() {
        assert_eq!(builtin_type("Int"), Some(Type::Int));
        assert_eq!(builtin_type("Nat"), Some(Type::Nat));
        assert_eq!(builtin_type("Float"), Some(Type::Float));
        assert_eq!(builtin_type("Bool"), Some(Type::Bool));
        assert_eq!(builtin_type("String"), Some(Type::String));
        assert_eq!(builtin_type("Bytes"), Some(Type::Bytes));
        assert_eq!(builtin_type("Unit"), Some(Type::Unit));
        assert_eq!(builtin_type("Never"), Some(Type::Never));
    }

    #[test]
    fn builtin_type_fixed_width() {
        assert_eq!(builtin_type("U8"), Some(Type::U8));
        assert_eq!(builtin_type("U16"), Some(Type::U16));
        assert_eq!(builtin_type("U32"), Some(Type::U32));
        assert_eq!(builtin_type("U64"), Some(Type::U64));
        assert_eq!(builtin_type("I8"), Some(Type::I8));
        assert_eq!(builtin_type("I64"), Some(Type::I64));
        assert_eq!(builtin_type("F32"), Some(Type::F32));
        assert_eq!(builtin_type("F64"), Some(Type::F64));
    }

    #[test]
    fn builtin_type_generic_containers_bare() {
        // Bare generic names produce Unknown inner types
        assert_eq!(
            builtin_type("List"),
            Some(Type::List(Box::new(Type::Unknown)))
        );
        assert_eq!(
            builtin_type("Set"),
            Some(Type::Set(Box::new(Type::Unknown)))
        );
        assert_eq!(
            builtin_type("Option"),
            Some(Type::Option(Box::new(Type::Unknown)))
        );
    }

    #[test]
    fn builtin_type_unknown_name() {
        assert_eq!(builtin_type("FooBar"), None);
        assert_eq!(builtin_type(""), None);
        assert_eq!(builtin_type("int"), None); // case-sensitive
    }

    // ---- TypeEnv ----

    #[test]
    fn type_env_insert_and_lookup() {
        let mut env = TypeEnv::new();
        assert!(env.is_empty());
        assert_eq!(env.len(), 0);

        env.insert("x".into(), Type::Int);
        assert_eq!(env.lookup("x"), Some(&Type::Int));
        assert_eq!(env.len(), 1);
        assert!(!env.is_empty());
    }

    #[test]
    fn type_env_insert_overwrites() {
        let mut env = TypeEnv::new();
        let prev = env.insert("x".into(), Type::Int);
        assert!(prev.is_none());

        let prev = env.insert("x".into(), Type::Bool);
        assert_eq!(prev, Some(Type::Int));
        assert_eq!(env.lookup("x"), Some(&Type::Bool));
    }

    #[test]
    fn type_env_lookup_missing() {
        let env = TypeEnv::new();
        assert_eq!(env.lookup("nonexistent"), None);
    }

    #[test]
    fn type_env_lookup_field() {
        let mut env = TypeEnv::new();
        env.struct_fields.insert(
            "Point".into(),
            vec![("x".into(), Type::Float), ("y".into(), Type::Float)],
        );
        assert_eq!(env.lookup_field("Point", "x"), Some(&Type::Float));
        assert_eq!(env.lookup_field("Point", "z"), None);
        assert_eq!(env.lookup_field("Unknown", "x"), None);
    }

    // ---- TypeError ----

    #[test]
    fn type_error_with_context() {
        let err = TypeError {
            code: "A03001".into(),
            message: "type mismatch".into(),
            span: 10..20,
            secondary: None,
            suggestion: None,
        };
        let enriched = err.with_context("in function foo");
        assert_eq!(enriched.message, "type mismatch (in function foo)");
        assert_eq!(enriched.span, 10..20);
    }

    #[test]
    fn type_error_to_diagnostic_with_explicit_suggestion() {
        let err = TypeError {
            code: "A03001".into(),
            message: "type mismatch".into(),
            span: 10..20,
            secondary: None,
            suggestion: Some("use `as Int` to cast".into()),
        };
        let diag: assura_diagnostics::Diagnostic = err.into();
        assert_eq!(diag.code, "A03001");
        let s = diag.suggestion.expect("should have suggestion");
        assert_eq!(s.message, "use `as Int` to cast");
    }

    #[test]
    fn type_error_to_diagnostic_falls_back_to_catalog() {
        // A03001 exists in the catalog with a non-empty fix field
        let err = TypeError {
            code: "A03001".into(),
            message: "type mismatch".into(),
            span: 0..5,
            secondary: None,
            suggestion: None,
        };
        let diag: assura_diagnostics::Diagnostic = err.into();
        // The catalog fallback should populate the suggestion
        let s = diag
            .suggestion
            .expect("catalog fallback should produce suggestion");
        assert!(
            !s.message.is_empty(),
            "catalog fix text should not be empty"
        );
    }

    /// #903: catalog Help for A03005 must be field-oriented (not "calling a function").
    #[test]
    fn a03005_catalog_help_is_field_oriented() {
        let err = TypeError {
            code: "A03005".into(),
            message: "tuple index `2` out of range for type `(Int, Bool)` (arity 2)".into(),
            span: 0..5,
            secondary: None,
            suggestion: None,
        };
        let diag: assura_diagnostics::Diagnostic = err.into();
        let s = diag
            .suggestion
            .expect("A03005 catalog should provide Help/suggestion");
        let help = s.message.to_lowercase();
        assert!(
            !help.contains("calling a function"),
            "A03005 Help must not mention calling a function, got: {}",
            s.message
        );
        assert!(
            help.contains("field") || help.contains("tuple"),
            "A03005 Help should be field-oriented, got: {}",
            s.message
        );
    }

    #[test]
    fn type_error_to_diagnostic_no_suggestion_for_unknown_code() {
        let err = TypeError {
            code: "A00000".into(),
            message: "unknown error".into(),
            span: 0..1,
            secondary: None,
            suggestion: None,
        };
        let diag: assura_diagnostics::Diagnostic = err.into();
        // A00000 is not in the catalog, so no suggestion
        assert!(diag.suggestion.is_none());
    }

    // ---- Scoped TypeEnv ----

    #[test]
    fn typeenv_global_scope_lookup() {
        let mut env = TypeEnv::new();
        env.insert("x".into(), Type::Int);
        assert_eq!(env.lookup("x"), Some(&Type::Int));
        assert_eq!(env.depth(), 0);
    }

    #[test]
    fn typeenv_push_pop_scope() {
        let mut env = TypeEnv::new();
        env.insert("x".into(), Type::Int);
        assert_eq!(env.depth(), 0);

        env.push_scope();
        assert_eq!(env.depth(), 1);
        // Can still see outer binding
        assert_eq!(env.lookup("x"), Some(&Type::Int));

        // Inner binding shadows outer
        env.insert("x".into(), Type::Bool);
        assert_eq!(env.lookup("x"), Some(&Type::Bool));

        env.pop_scope();
        assert_eq!(env.depth(), 0);
        // Shadowing removed, original type restored
        assert_eq!(env.lookup("x"), Some(&Type::Int));
    }

    #[test]
    fn typeenv_nested_scopes() {
        let mut env = TypeEnv::new();
        env.insert("a".into(), Type::Int);

        env.push_scope();
        env.insert("b".into(), Type::Bool);

        env.push_scope();
        env.insert("c".into(), Type::String);

        // All visible from innermost scope
        assert_eq!(env.lookup("a"), Some(&Type::Int));
        assert_eq!(env.lookup("b"), Some(&Type::Bool));
        assert_eq!(env.lookup("c"), Some(&Type::String));
        assert_eq!(env.depth(), 2);

        env.pop_scope();
        assert!(env.lookup("c").is_none());
        assert_eq!(env.lookup("b"), Some(&Type::Bool));

        env.pop_scope();
        assert!(env.lookup("b").is_none());
        assert_eq!(env.lookup("a"), Some(&Type::Int));
    }

    #[test]
    fn typeenv_inner_binding_does_not_leak() {
        let mut env = TypeEnv::new();
        env.push_scope();
        env.insert("local".into(), Type::Nat);
        assert_eq!(env.lookup("local"), Some(&Type::Nat));
        env.pop_scope();
        assert!(env.lookup("local").is_none());
    }

    #[test]
    fn typeenv_len_counts_all_scopes() {
        let mut env = TypeEnv::new();
        env.insert("x".into(), Type::Int);
        env.push_scope();
        env.insert("y".into(), Type::Bool);
        assert_eq!(env.len(), 2);
        env.pop_scope();
        assert_eq!(env.len(), 1);
    }

    #[test]
    fn typeenv_is_empty_across_scopes() {
        let mut env = TypeEnv::new();
        assert!(env.is_empty());
        env.push_scope();
        assert!(env.is_empty());
        env.insert("x".into(), Type::Int);
        assert!(!env.is_empty());
        env.pop_scope();
        assert!(env.is_empty());
    }

    #[test]
    #[should_panic(expected = "cannot pop the global scope")]
    fn typeenv_pop_global_panics() {
        let mut env = TypeEnv::new();
        env.pop_scope(); // should panic
    }

    #[test]
    fn typeenv_struct_fields_not_scope_dependent() {
        let mut env = TypeEnv::new();
        env.struct_fields
            .insert("Point".into(), vec![("x".into(), Type::Float)]);
        env.push_scope();
        assert_eq!(env.lookup_field("Point", "x"), Some(&Type::Float));
        env.pop_scope();
        assert_eq!(env.lookup_field("Point", "x"), Some(&Type::Float));
    }

    // ---- Refined type with Expr predicate ----

    #[test]
    fn refined_from_str_creates_expr_raw() {
        let ty = Type::refined_from_str(Type::Int, "x", "x > 0");
        if let Type::Refined {
            base,
            predicate,
            bound_var,
        } = &ty
        {
            assert_eq!(**base, Type::Int);
            assert_eq!(bound_var, "x");
            // Predicate should be Expr::Raw with split tokens
            assert!(
                matches!(&predicate.node, Expr::Raw(tokens) if tokens.len() == 3),
                "expected 3-token Raw, got {:?}",
                predicate.node
            );
        } else {
            panic!("expected Refined");
        }
    }

    #[test]
    fn refined_predicate_str_returns_text() {
        let ty = Type::refined_from_str(Type::Nat, "v", "v >= 0");
        assert_eq!(ty.predicate_str(), Some("v >= 0".into()));
    }

    #[test]
    fn refined_predicate_str_empty_returns_none() {
        let ty = Type::refined_from_str(Type::Int, "x", "");
        assert_eq!(ty.predicate_str(), None);
    }

    #[test]
    fn refined_display_uses_bound_var() {
        let ty = Type::refined_from_str(Type::Int, "v", "v > 0");
        assert_eq!(ty.to_string(), "{ v : Int | v > 0 }");
    }

    #[test]
    fn refined_with_structural_expr() {
        // Construct a Refined type with a real BinOp expression
        use assura_parser::ast::BinOp;
        let pred = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            op: BinOp::Gt,
            rhs: Box::new(Spanned::no_span(Expr::Literal(
                assura_parser::ast::Literal::Int("0".into()),
            ))),
        });
        let ty = Type::Refined {
            base: Box::new(Type::Int),
            predicate: Box::new(pred),
            bound_var: "x".into(),
        };
        // Should display as { x : Int | x > 0 }
        let s = ty.to_string();
        assert!(s.contains("x") && s.contains("Int"), "got: {s}");
    }

    #[test]
    fn refined_non_refined_type_predicate_str_is_none() {
        assert_eq!(Type::Int.predicate_str(), None);
        assert_eq!(Type::Bool.predicate_str(), None);
    }
}
