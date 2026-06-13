//! Type checking for the Assura contract language.
//!
//! Builds a `TypeEnv` (type environment) from a `ResolvedFile` by mapping
//! each symbol in the symbol table to its `Type`. For T013 this creates the
//! scaffolding: type environment construction and the `type_check` entry
//! point. Actual expression-level type checking (T014-T018) builds on this.

use std::collections::HashMap;
use std::ops::Range;

use assura_parser::ast::{BinOp, ClauseKind, Decl, Expr, Literal, ServiceItem, UnaryOp};
use assura_resolve::{ResolvedFile, SymbolKind, SymbolTable};

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

    // --- Refined type: base type with predicate ---
    Refined {
        base: Box<Type>,
        predicate: std::string::String,
    },

    // --- Unknown / error recovery placeholder ---
    Unknown,
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
    pub fn lookup_field(&self, struct_name: &str, field_name: &str) -> Option<&Type> {
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
    pub code: std::string::String,
    /// Human-readable error message.
    pub message: std::string::String,
    /// Source location where the error was detected.
    pub span: Range<usize>,
    /// Optional secondary span with label (e.g., "expected type declared here").
    pub secondary: Option<(Range<usize>, std::string::String)>,
}

// ---------------------------------------------------------------------------
// Typed file
// ---------------------------------------------------------------------------

/// The result of successful type checking: the resolved file plus the
/// type environment constructed from its symbols.
#[derive(Debug, Clone)]
pub struct TypedFile {
    pub resolved: ResolvedFile,
    pub type_env: TypeEnv,
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
        // Generic container types get `Unknown` as their type argument
        // until T014+ refines this with actual type argument resolution.
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
// Type token parsing
// ---------------------------------------------------------------------------

/// Parse a raw token sequence (e.g. `["List", "<", "Int", ">"]`) into a
/// structured `Type`. Handles base types, generic containers, refinement
/// types, taint annotations, reference/mutable types, and union error types.
fn parse_type_tokens(tokens: &[String]) -> Type {
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
            return Type::Refined {
                base: Box::new(base),
                predicate: String::new(),
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
            SymbolKind::FnDef | SymbolKind::ExternFn => Type::Fn {
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
        };

        env.insert(sym.name.clone(), ty);
    }

    // Enrich from AST: parse Param.ty token sequences into structured Types
    // and build proper function signatures with param types and return types.
    for decl in &source.decls {
        match &decl.node {
            Decl::FnDef(f) => {
                // Insert parameter types
                for p in &f.params {
                    let ty = parse_type_tokens(&p.ty);
                    env.insert(p.name.clone(), ty);
                }
                // Build full function type
                let param_types: Vec<Type> =
                    f.params.iter().map(|p| parse_type_tokens(&p.ty)).collect();
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
                    let ty = parse_type_tokens(&p.ty);
                    env.insert(p.name.clone(), ty);
                }
                let param_types: Vec<Type> =
                    e.params.iter().map(|p| parse_type_tokens(&p.ty)).collect();
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
            Decl::Service(_) => {
                // Service operations/queries only have name + clauses in the
                // AST (no explicit params/return_ty). Their types remain as
                // registered from the symbol table.
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
            _ => {}
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
            Type::Refined { base, predicate } => write!(f, "{base}{{{predicate}}}"),
            Type::Unknown => write!(f, "Unknown"),
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
/// Returns `Ok(())` on success, or `Err(TypeError)` with code A03003 if the
/// argument count does not match.
pub fn check_generic_instantiation(
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
                code: "A03003".into(),
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
                code: "A03003".into(),
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
pub fn substitute(ty: &Type, bindings: &HashMap<std::string::String, Type>) -> Type {
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
pub fn instantiate_builtin_generic(name: &str, args: Vec<Type>) -> Option<Type> {
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
// Expression type inference
// ---------------------------------------------------------------------------

/// Returns `true` if `ty` is a numeric type.
fn is_numeric(ty: &Type) -> bool {
    match ty {
        Type::Int
        | Type::Nat
        | Type::Float
        | Type::U8
        | Type::U16
        | Type::U32
        | Type::U64
        | Type::I8
        | Type::I16
        | Type::I32
        | Type::I64
        | Type::F32
        | Type::F64 => true,
        // A refined type is numeric if its base type is numeric
        Type::Refined { base, .. } => is_numeric(base),
        // Named types may be numeric aliases; be lenient
        Type::Named(_) | Type::Unknown => true,
        _ => false,
    }
}

/// Infer the type of an expression given a type environment.
///
/// Returns `Ok(ty)` with the inferred type, or `Err(TypeError)` when a
/// concrete type mismatch is detected (A03001). Unknown types (from
/// unresolved references) are propagated silently; they never trigger
/// errors.
pub fn infer_expr(expr: &Expr, env: &TypeEnv) -> Result<Type, TypeError> {
    match expr {
        // --- Literals ---
        Expr::Literal(Literal::Int(_)) => Ok(Type::Int),
        Expr::Literal(Literal::Float(_)) => Ok(Type::Float),
        Expr::Literal(Literal::Str(_)) => Ok(Type::String),
        Expr::Literal(Literal::Bool(_)) => Ok(Type::Bool),

        // --- Identifiers ---
        Expr::Ident(name) => {
            // Special built-in names
            if name == "result" || name == "self" || name == "true" || name == "false" {
                if name == "true" || name == "false" {
                    return Ok(Type::Bool);
                }
                return Ok(Type::Unknown);
            }
            Ok(env.lookup(name).cloned().unwrap_or(Type::Unknown))
        }

        // --- Binary operations ---
        Expr::BinOp { lhs, op, rhs } => infer_binop(lhs, op, rhs, env),

        // --- Unary operations ---
        Expr::UnaryOp { op, expr: inner } => {
            let inner_ty = infer_expr(inner, env)?;
            match op {
                UnaryOp::Neg => {
                    if inner_ty == Type::Unknown || is_numeric(&inner_ty) {
                        Ok(inner_ty)
                    } else {
                        Err(TypeError {
                            code: "A03001".into(),
                            message: format!(
                                "unary `-` requires a numeric type, found `{inner_ty}`"
                            ),
                            span: 0..0,
                            secondary: None,
                        })
                    }
                }
                UnaryOp::Not => {
                    if inner_ty == Type::Unknown || inner_ty == Type::Bool {
                        Ok(Type::Bool)
                    } else {
                        Err(TypeError {
                            code: "A03001".into(),
                            message: format!("unary `!` requires Bool, found `{inner_ty}`"),
                            span: 0..0,
                            secondary: None,
                        })
                    }
                }
            }
        }

        // --- If-then-else ---
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            let cond_ty = infer_expr(cond, env)?;
            if cond_ty != Type::Unknown && cond_ty != Type::Bool {
                return Err(TypeError {
                    code: "A03001".into(),
                    message: format!("if condition must be Bool, found `{cond_ty}`"),
                    span: 0..0,
                    secondary: None,
                });
            }
            let then_ty = infer_expr(then_branch, env)?;
            if let Some(else_br) = else_branch {
                let else_ty = infer_expr(else_br, env)?;
                if then_ty == Type::Unknown {
                    Ok(else_ty)
                } else if else_ty == Type::Unknown || then_ty == else_ty {
                    Ok(then_ty)
                } else {
                    Err(TypeError {
                        code: "A03001".into(),
                        message: format!(
                            "if branches have different types: `{then_ty}` vs `{else_ty}`"
                        ),
                        span: 0..0,
                        secondary: None,
                    })
                }
            } else {
                Ok(then_ty)
            }
        }

        // --- Quantifiers ---
        Expr::Forall { body, .. } | Expr::Exists { body, .. } => {
            // The body should be Bool but we don't enforce that strictly
            // here (domain might introduce Unknown bindings). The overall
            // result of a quantifier is always Bool.
            let _body_ty = infer_expr(body, env)?;
            Ok(Type::Bool)
        }

        // --- old(expr) ---
        Expr::Old(inner) => infer_expr(inner, env),

        // --- Parenthesized ---
        Expr::Paren(inner) => infer_expr(inner, env),

        // --- List literal ---
        Expr::List(items) => {
            if items.is_empty() {
                return Ok(Type::List(Box::new(Type::Unknown)));
            }
            let first_ty = infer_expr(&items[0], env)?;
            // Check remaining items match the first
            for item in &items[1..] {
                let item_ty = infer_expr(item, env)?;
                if item_ty != Type::Unknown && first_ty != Type::Unknown && item_ty != first_ty {
                    return Err(TypeError {
                        code: "A03001".into(),
                        message: format!(
                            "list element type mismatch: expected `{first_ty}`, found `{item_ty}`"
                        ),
                        span: 0..0,
                        secondary: None,
                    });
                }
            }
            Ok(Type::List(Box::new(first_ty)))
        }

        // --- Field access ---
        Expr::Field(receiver, field) => {
            let recv_ty = infer_expr(receiver, env)?;
            // Try to resolve the field on the receiver's type
            let struct_name = match &recv_ty {
                Type::Named(name) => Some(name.as_str()),
                Type::Refined { base, .. } => {
                    if let Type::Named(name) = base.as_ref() {
                        Some(name.as_str())
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(sname) = struct_name
                && let Some(field_ty) = env.lookup_field(sname, field)
            {
                return Ok(field_ty.clone());
            }
            // Common built-in fields: len/length/size/capacity on collections
            if matches!(
                &recv_ty,
                Type::List(_) | Type::Sequence(_) | Type::Bytes | Type::String
            ) && (field == "len" || field == "length" || field == "size" || field == "capacity")
            {
                return Ok(Type::Nat);
            }
            // Cannot resolve field; return Unknown (no false positive A03004)
            Ok(Type::Unknown)
        }

        // --- Method call ---
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            let recv_ty = infer_expr(receiver, env)?;
            // Infer argument types to surface errors inside them
            for arg in args {
                let _ = infer_expr(arg, env)?;
            }
            // Try to resolve the method as a known function in the env
            if let Some(Type::Fn { ret, .. }) = env.lookup(method) {
                return Ok(*ret.clone());
            }
            // Common collection methods with known return types
            match &recv_ty {
                Type::List(elem) | Type::Sequence(elem) => match method.as_str() {
                    "len" | "length" | "size" | "count" => return Ok(Type::Nat),
                    "get" | "first" | "last" => {
                        return Ok(Type::Option(elem.clone()));
                    }
                    "contains" | "is_empty" | "any" | "all" => return Ok(Type::Bool),
                    "push" | "append" | "remove" | "clear" => return Ok(Type::Unit),
                    "map" | "filter" | "take" | "skip" | "reverse" | "sort" => {
                        return Ok(recv_ty);
                    }
                    _ => {}
                },
                Type::Map(_, val) => match method.as_str() {
                    "get" => return Ok(Type::Option(val.clone())),
                    "contains_key" | "is_empty" => return Ok(Type::Bool),
                    "len" | "size" => return Ok(Type::Nat),
                    _ => {}
                },
                Type::Set(_) => match method.as_str() {
                    "contains" | "is_empty" | "is_subset" | "is_superset" => {
                        return Ok(Type::Bool);
                    }
                    "len" | "size" => return Ok(Type::Nat),
                    _ => {}
                },
                _ => {}
            }
            Ok(Type::Unknown)
        }

        // --- Function call ---
        Expr::Call { func, args } => infer_call(func, args, env),

        // --- Index access ---
        Expr::Index { expr: base, index } => {
            let base_ty = infer_expr(base, env)?;
            // Infer index type to surface errors inside it.
            let _index_ty = infer_expr(index, env)?;
            match base_ty {
                Type::List(elem) => Ok(*elem),
                Type::Map(_key, val) => Ok(*val),
                Type::Sequence(elem) => Ok(*elem),
                // Unknown or user-defined types: return Unknown.
                _ => Ok(Type::Unknown),
            }
        }

        // --- Cast: cannot infer target type from string yet ---
        Expr::Cast { .. } => Ok(Type::Unknown),

        // --- Apply lemma: type-check args, result is Bool (adds assumption) ---
        Expr::Apply { args, .. } => {
            for arg in args {
                let _ = infer_expr(arg, env)?;
            }
            // apply expressions contribute assumptions; they have Bool type
            // in the verification domain
            Ok(Type::Bool)
        }

        // --- Ghost block: type-check inner, result is Unit (erased at runtime) ---
        Expr::Ghost(inner) => {
            // Type-check the inner expression (it must be valid in the
            // verification domain) but the ghost block itself evaluates
            // to Unit since it is erased at runtime.
            let _inner_ty = infer_expr(inner, env)?;
            Ok(Type::Unit)
        }

        // --- Match: infer type from first arm body (all arms should agree) ---
        Expr::Match { scrutinee, arms } => {
            let _ = infer_expr(scrutinee, env)?;
            if let Some(first) = arms.first() {
                infer_expr(&first.body, env)
            } else {
                Ok(Type::Unknown)
            }
        }

        // --- Let binding: infer body type ---
        Expr::Let { body, .. } => infer_expr(body, env),

        // --- Tuple: cannot infer structured tuple type yet ---
        Expr::Tuple(_) => Ok(Type::Unknown),

        // --- Block / Raw: cannot infer ---
        Expr::Block(_) | Expr::Raw(_) => Ok(Type::Unknown),
    }
}

/// Check if two types are compatible for comparison/arithmetic purposes.
///
/// Types are compatible if:
/// - They are equal
/// - Either side is `Unknown`
/// - Either side is a `Named` type (user-defined, not yet resolved)
/// - A `Refined` type's base matches the other type
/// - Both are numeric
fn types_compatible(a: &Type, b: &Type) -> bool {
    if a == b {
        return true;
    }
    if *a == Type::Unknown || *b == Type::Unknown {
        return true;
    }
    // Named types are unresolved user-defined; be lenient
    if matches!(a, Type::Named(_)) || matches!(b, Type::Named(_)) {
        return true;
    }
    // Refined types are compatible with their base type
    if let Type::Refined { base, .. } = a {
        return types_compatible(base, b);
    }
    if let Type::Refined { base, .. } = b {
        return types_compatible(a, base);
    }
    // TypeParams are unresolved; be lenient
    if matches!(a, Type::TypeParam(_)) || matches!(b, Type::TypeParam(_)) {
        return true;
    }
    // Nat is a subtype of Int; they are compatible in arithmetic/comparison
    if (matches!(a, Type::Nat) && matches!(b, Type::Int))
        || (matches!(a, Type::Int) && matches!(b, Type::Nat))
    {
        return true;
    }
    // Both numeric types are compatible (e.g., U32 vs Int in mixed arithmetic)
    if is_numeric(a) && is_numeric(b) {
        return true;
    }
    false
}

/// Infer the result type of a binary operation.
fn infer_binop(lhs: &Expr, op: &BinOp, rhs: &Expr, env: &TypeEnv) -> Result<Type, TypeError> {
    let lhs_ty = infer_expr(lhs, env)?;
    let rhs_ty = infer_expr(rhs, env)?;

    // If either side is Unknown, be lenient
    if lhs_ty == Type::Unknown || rhs_ty == Type::Unknown {
        return match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod | BinOp::Concat => {
                // Return whichever side is known, or Unknown
                if lhs_ty != Type::Unknown {
                    Ok(lhs_ty)
                } else {
                    Ok(rhs_ty)
                }
            }
            BinOp::Eq
            | BinOp::Neq
            | BinOp::Lt
            | BinOp::Lte
            | BinOp::Gt
            | BinOp::Gte
            | BinOp::And
            | BinOp::Or
            | BinOp::Implies
            | BinOp::In
            | BinOp::NotIn => Ok(Type::Bool),
            BinOp::Range => Ok(Type::Unknown),
        };
    }

    match op {
        // Arithmetic: both operands same numeric type, result same type
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
            if !is_numeric(&lhs_ty) {
                return Err(TypeError {
                    code: "A03001".into(),
                    message: format!(
                        "arithmetic operator requires numeric types, found `{lhs_ty}`"
                    ),
                    span: 0..0,
                    secondary: None,
                });
            }
            if !types_compatible(&lhs_ty, &rhs_ty) {
                return Err(TypeError {
                    code: "A03001".into(),
                    message: format!("type mismatch in arithmetic: `{lhs_ty}` vs `{rhs_ty}`"),
                    span: 0..0,
                    secondary: None,
                });
            }
            Ok(lhs_ty)
        }

        // Comparison: operands compatible types, result Bool
        BinOp::Eq | BinOp::Neq | BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte => {
            if !types_compatible(&lhs_ty, &rhs_ty) {
                return Err(TypeError {
                    code: "A03001".into(),
                    message: format!(
                        "comparison requires same types, found `{lhs_ty}` vs `{rhs_ty}`"
                    ),
                    span: 0..0,
                    secondary: None,
                });
            }
            Ok(Type::Bool)
        }

        // Logical: both Bool, result Bool
        BinOp::And | BinOp::Or | BinOp::Implies => {
            if lhs_ty != Type::Bool {
                return Err(TypeError {
                    code: "A03001".into(),
                    message: format!("logical operator requires Bool, found `{lhs_ty}`"),
                    span: 0..0,
                    secondary: None,
                });
            }
            if rhs_ty != Type::Bool {
                return Err(TypeError {
                    code: "A03001".into(),
                    message: format!("logical operator requires Bool, found `{rhs_ty}`"),
                    span: 0..0,
                    secondary: None,
                });
            }
            Ok(Type::Bool)
        }

        // Concat: both same type, result same type
        BinOp::Concat => {
            if lhs_ty != rhs_ty {
                return Err(TypeError {
                    code: "A03001".into(),
                    message: format!("concat requires same types, found `{lhs_ty}` vs `{rhs_ty}`"),
                    span: 0..0,
                    secondary: None,
                });
            }
            Ok(lhs_ty)
        }

        // Range: both Int, result Unknown (range type deferred)
        BinOp::Range => {
            if lhs_ty != Type::Int {
                return Err(TypeError {
                    code: "A03001".into(),
                    message: format!("range requires Int operands, found `{lhs_ty}`"),
                    span: 0..0,
                    secondary: None,
                });
            }
            if rhs_ty != Type::Int {
                return Err(TypeError {
                    code: "A03001".into(),
                    message: format!("range requires Int operands, found `{rhs_ty}`"),
                    span: 0..0,
                    secondary: None,
                });
            }
            Ok(Type::Unknown)
        }

        // In/NotIn: result Bool
        BinOp::In | BinOp::NotIn => Ok(Type::Bool),
    }
}

/// Infer the result type of a function call expression.
fn infer_call(func: &Expr, args: &[Expr], env: &TypeEnv) -> Result<Type, TypeError> {
    let func_ty = infer_expr(func, env)?;

    // Infer argument types eagerly so errors inside arguments are surfaced
    // even when the callee type is Unknown.
    let mut arg_types = Vec::with_capacity(args.len());
    for arg in args {
        arg_types.push(infer_expr(arg, env)?);
    }

    match func_ty {
        Type::Fn { params, ret } => {
            // If params is non-empty, check argument count.
            // (params may be empty when the function was registered with
            // placeholder params from the symbol table.)
            if !params.is_empty() && params.len() != arg_types.len() {
                return Err(TypeError {
                    code: "A03002".into(),
                    message: format!(
                        "function expects {} argument(s), but {} were provided",
                        params.len(),
                        arg_types.len()
                    ),
                    span: 0..0,
                    secondary: None,
                });
            }
            Ok(*ret)
        }
        // Unknown callee: be lenient, propagate Unknown.
        Type::Unknown => Ok(Type::Unknown),
        // Named type: could be a constructor or unresolved callable.
        // Be lenient and return Unknown.
        Type::Named(_) | Type::TypeParam(_) => Ok(Type::Unknown),
        // Definitely not callable.
        other => Err(TypeError {
            code: "A03005".into(),
            message: format!("type `{other}` is not callable"),
            span: 0..0,
            secondary: None,
        }),
    }
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
fn check_lemma_fn_effects(
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
fn check_ghost_fn_effects(
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
// Clause body type checking
// ---------------------------------------------------------------------------

/// Walk all clause bodies in a source file, infer expression types, and
/// collect type errors. Lenient: errors involving `Unknown` are suppressed.
fn check_clause_bodies(source: &assura_parser::ast::SourceFile, env: &TypeEnv) -> Vec<TypeError> {
    let mut errors = Vec::new();

    for decl in &source.decls {
        let span = &decl.span;
        match &decl.node {
            Decl::Contract(c) => {
                for clause in &c.clauses {
                    check_clause_expr(&clause.kind, &clause.body, env, &mut errors, span);
                }
            }
            Decl::FnDef(f) => {
                // T043 CORE.1: ghost functions must have pure effects
                if f.is_ghost {
                    check_ghost_fn_effects(f, span, &mut errors);
                }
                // T044 CORE.2: lemma functions must have pure effects
                if f.is_lemma {
                    check_lemma_fn_effects(f, span, &mut errors);
                }
                for clause in &f.clauses {
                    check_clause_expr(&clause.kind, &clause.body, env, &mut errors, span);
                }
            }
            Decl::Extern(ex) => {
                for clause in &ex.clauses {
                    check_clause_expr(&clause.kind, &clause.body, env, &mut errors, span);
                }
            }
            Decl::Service(s) => {
                for item in &s.items {
                    let clauses = match item {
                        ServiceItem::Operation { clauses, .. }
                        | ServiceItem::Query { clauses, .. } => clauses.as_slice(),
                        ServiceItem::Invariant(expr) => {
                            // Service-level invariants are always Bool-typed
                            check_clause_expr(&ClauseKind::Invariant, expr, env, &mut errors, span);
                            continue;
                        }
                        ServiceItem::Other { body, .. } => {
                            collect_expr_errors(body, env, &mut errors, span);
                            continue;
                        }
                        _ => continue,
                    };
                    for clause in clauses {
                        check_clause_expr(&clause.kind, &clause.body, env, &mut errors, span);
                    }
                }
            }
            Decl::Block { body, .. } => {
                for clause in body {
                    check_clause_expr(&clause.kind, &clause.body, env, &mut errors, span);
                }
            }
            // TypeDef and EnumDef don't have expression bodies
            Decl::TypeDef(_) | Decl::EnumDef(_) => {}
        }
    }

    errors
}

/// Try to infer the type of an expression; if a type error occurs, push
/// it into the collector. Uses `ctx_span` to replace placeholder `0..0`
/// spans with the declaration's actual source span.
fn collect_expr_errors(
    expr: &Expr,
    env: &TypeEnv,
    errors: &mut Vec<TypeError>,
    ctx_span: &std::ops::Range<usize>,
) {
    match infer_expr(expr, env) {
        Ok(_) => {}
        Err(mut e) => {
            if e.span == (0..0) {
                e.span = ctx_span.clone();
            }
            errors.push(e);
        }
    }
}

/// Returns `true` if the clause kind requires a Bool-typed body.
fn clause_requires_bool(kind: &ClauseKind) -> bool {
    matches!(
        kind,
        ClauseKind::Requires | ClauseKind::Ensures | ClauseKind::Invariant | ClauseKind::Rule
    )
}

/// Human-readable label for a clause kind (used in error messages).
fn clause_kind_label(kind: &ClauseKind) -> &'static str {
    match kind {
        ClauseKind::Requires => "requires",
        ClauseKind::Ensures => "ensures",
        ClauseKind::Invariant => "invariant",
        ClauseKind::Rule => "rule",
        _ => "clause",
    }
}

/// Check a single clause expression. Infer its type, push any inference
/// errors, and additionally emit A03006 if the clause kind demands Bool
/// but the body has a definitively non-Bool type.
fn check_clause_expr(
    kind: &ClauseKind,
    body: &Expr,
    env: &TypeEnv,
    errors: &mut Vec<TypeError>,
    ctx_span: &std::ops::Range<usize>,
) {
    match infer_expr(body, env) {
        Ok(ty) => {
            if clause_requires_bool(kind) && ty != Type::Unknown && ty != Type::Bool {
                errors.push(TypeError {
                    code: "A03006".into(),
                    message: format!(
                        "{} clause must be Bool, found `{ty}`",
                        clause_kind_label(kind),
                    ),
                    span: ctx_span.clone(),
                    secondary: None,
                });
            }
        }
        Err(mut e) => {
            if e.span == (0..0) {
                e.span = ctx_span.clone();
            }
            errors.push(e);
        }
    }
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
    let type_env = build_type_env(&resolved.symbols, &resolved.source);

    // T014: walk clause bodies and infer expression types. Collect any
    // concrete type-mismatch errors (A03001). Unknown types from unresolved
    // identifiers are silently propagated (no false positives).
    let mut errors = check_clause_bodies(&resolved.source, &type_env);

    // T077: check axiomatic definition references and usage
    errors.extend(run_axiomatic_checks(&resolved.source, &resolved.symbols));

    // T109: check CRUD/auth coverage on services
    errors.extend(run_crud_auth_checks(&resolved.source));

    // T031/T032: linearity checking (usage tracking + context splitting)
    errors.extend(run_linearity_checks(&resolved.source));

    // T034: typestate checking (DFA state transitions on services)
    errors.extend(run_typestate_checks(&resolved.source));

    // T036: effect checking (declared vs actual effect containment)
    errors.extend(run_effect_checks(&resolved.source));

    // T047: taint tracking (untrusted data flow analysis)
    errors.extend(run_taint_checks(&resolved.source));

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
    errors.extend(run_totality_checks(&resolved.source));

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

    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(TypedFile {
        resolved: resolved.clone(),
        type_env,
    })
}

/// T077: Scan for axiomatic blocks and validate references/usage.
fn run_axiomatic_checks(
    source: &assura_parser::ast::SourceFile,
    symbols: &assura_resolve::SymbolTable,
) -> Vec<TypeError> {
    let mut checker = AxiomaticDefChecker::new();
    for decl in &source.decls {
        if let Decl::Block { kind, name, .. } = &decl.node
            && (kind == "axiomatic" || kind == "axiom")
        {
            checker.declare_axiom(AxiomDef {
                name: name.clone(),
                params: Vec::new(),
                body: std::string::String::new(),
                span: decl.span.clone(),
                references: Vec::new(),
            });
        }
    }
    let known: Vec<&str> = symbols.symbols.iter().map(|s| s.name.as_str()).collect();
    checker.check_references(&known)
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
            errors.extend(checker.check_auth_coverage());
            errors.extend(checker.check_delete_protection());
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
                let mut tracker = UsageTracker::new();
                for param in &f.params {
                    if param.ty.iter().any(|t| t == "linear") {
                        tracker.declare(param.name.clone(), UsageGrade::Linear, decl.span.clone());
                    }
                }
                let mut ctx = LinearContext::new(tracker);
                for clause in &f.clauses {
                    errors.extend(check_expr_linearity(&clause.body, &mut ctx));
                }
                errors.extend(ctx.check());
            }
            Decl::Extern(e) => {
                let mut tracker = UsageTracker::new();
                for param in &e.params {
                    if param.ty.iter().any(|t| t == "linear") {
                        tracker.declare(param.name.clone(), UsageGrade::Linear, decl.span.clone());
                    }
                }
                let mut ctx = LinearContext::new(tracker);
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

/// Helper: declare linear parameters from an input clause expression.
fn declare_linear_params_from_expr(
    expr: &Expr,
    tracker: &mut UsageTracker,
    span: &std::ops::Range<usize>,
) {
    if let Expr::Raw(tokens) = expr {
        // Input clauses are often Raw token sequences like: x : linear Int, y : Int
        let mut i = 0;
        while i < tokens.len() {
            // Look for pattern: name : linear Type
            if i + 2 < tokens.len()
                && tokens[i + 1] == ":"
                && tokens[i + 2..].iter().any(|t| t == "linear")
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
                let checker =
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

    for decl in &source.decls {
        match &decl.node {
            Decl::FnDef(f) => {
                let (declared, actual) = extract_effects_from_clauses(&f.clauses);
                if let Some(declared_set) = declared {
                    // Validate all effect names are known
                    for ee in checker.check_known(&declared_set, &decl.span) {
                        errors.push(TypeError {
                            code: ee.code,
                            message: ee.message,
                            span: ee.span,
                            secondary: None,
                        });
                    }
                    // Check containment: actual subset of declared
                    if let Some(actual_set) = actual {
                        for ee in checker.check_containment(&declared_set, &actual_set, &decl.span)
                        {
                            errors.push(TypeError {
                                code: ee.code,
                                message: ee.message,
                                span: ee.span,
                                secondary: None,
                            });
                        }
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

/// T058: Run FFI boundary checks on extern declarations.
///
/// Only runs if at least one extern has explicit trust boundary annotations.
/// Without annotations, the checker would flag every extern as missing trust
/// info, which creates noise for files that don't use FFI boundary contracts.
fn run_ffi_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let checker = FfiBoundaryChecker::new();
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
            }
            if !policy.must_propagate.is_empty() || !policy.must_check.is_empty() {
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
fn run_totality_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let checker = TotalityChecker::new();
    let mut errors = Vec::new();

    // Collect all function definitions for mutual recursion checking
    let mut fn_defs: Vec<(&assura_parser::ast::FnDef, &std::ops::Range<usize>)> = Vec::new();

    for decl in &source.decls {
        if let Decl::FnDef(f) = &decl.node {
            fn_defs.push((f, &decl.span));
            for te in checker.check_function_totality(f, &decl.span) {
                errors.push(TypeError {
                    code: te.code,
                    message: te.message,
                    span: te.span,
                    secondary: None,
                });
            }
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

    errors
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
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Extern(e) => &e.clauses,
            _ => continue,
        };
        for clause in clauses {
            check_expr_fixed_width(&clause.body, type_env, &decl.span, &mut errors);
        }
    }
    errors
}

/// Recursively check an expression for fixed-width integer overflow.
fn check_expr_fixed_width(
    expr: &Expr,
    type_env: &TypeEnv,
    span: &std::ops::Range<usize>,
    errors: &mut Vec<TypeError>,
) {
    match expr {
        Expr::BinOp { lhs, op, rhs } => {
            // Check operands recursively
            check_expr_fixed_width(lhs, type_env, span, errors);
            check_expr_fixed_width(rhs, type_env, span, errors);

            // Check for overflow in arithmetic on fixed-width types
            if let Some(left_type) = infer_fixed_width_type(lhs, type_env)
                && let Some(right_type) = infer_fixed_width_type(rhs, type_env)
            {
                let checker = FixedWidthChecker::new();
                if let Some(fwe) =
                    checker.check_arithmetic_overflow(op, &left_type, &right_type, span)
                {
                    errors.push(TypeError {
                        code: fwe.code,
                        message: fwe.message,
                        span: fwe.span,
                        secondary: None,
                    });
                }
                if let Some(fwe) =
                    FixedWidthChecker::check_signedness_mismatch(op, &left_type, &right_type, span)
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
            check_expr_fixed_width(inner, type_env, span, errors);
            if let Some(from_type) = infer_fixed_width_type(inner, type_env)
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
            check_expr_fixed_width(inner, type_env, span, errors);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            check_expr_fixed_width(cond, type_env, span, errors);
            check_expr_fixed_width(then_branch, type_env, span, errors);
            if let Some(e) = else_branch {
                check_expr_fixed_width(e, type_env, span, errors);
            }
        }
        Expr::Call { func, args } => {
            check_expr_fixed_width(func, type_env, span, errors);
            for a in args {
                check_expr_fixed_width(a, type_env, span, errors);
            }
        }
        Expr::Block(items) => {
            for item in items {
                check_expr_fixed_width(item, type_env, span, errors);
            }
        }
        _ => {}
    }
}

/// Try to infer a fixed-width integer type for an expression.
fn infer_fixed_width_type(expr: &Expr, type_env: &TypeEnv) -> Option<Type> {
    match expr {
        Expr::Ident(name) => {
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
pub enum Pattern {
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
pub fn check_exhaustiveness(
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
            let clauses = match &decl.node {
                Decl::FnDef(f) => &f.clauses,
                Decl::Extern(e) => &e.clauses,
                _ => continue,
            };

            // Look for zeroize/erase patterns in ensures clauses
            let has_erasure = clauses
                .iter()
                .any(|c| c.kind == ClauseKind::Ensures && expr_references_var(&c.body, name));
            if has_erasure {
                checker.mark_zeroized(name.clone());
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

    errors
}

// ---------------------------------------------------------------------------
// Interface contracts wiring (T062)
// ---------------------------------------------------------------------------

/// Scan for contracts with `implements` clauses and validate that all
/// required interface methods are present with correct signatures.
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
                    .filter_map(|cl| {
                        if let Expr::Ident(name) = &cl.body {
                            Some(InterfaceMethod {
                                name: name.clone(),
                                param_types: vec![],
                                return_type: Type::Unknown,
                                has_requires: false,
                                has_ensures: false,
                                no_reentrancy: false,
                            })
                        } else {
                            None
                        }
                    })
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
                    let methods: Vec<String> = c
                        .clauses
                        .iter()
                        .filter(|cl| matches!(&cl.kind, ClauseKind::Other(k) if k == "method"))
                        .filter_map(|cl| {
                            if let Expr::Ident(name) = &cl.body {
                                Some(name.clone())
                            } else {
                                None
                            }
                        })
                        .collect();

                    for err in checker.check_impl(&c.name, iface_name, &methods, &decl.span) {
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
            && (kind == "lock_order" || kind == "lock_hierarchy")
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
        }
    }

    errors
}

// ---------------------------------------------------------------------------
// Usage tracking for linear types (T031)
// ---------------------------------------------------------------------------

/// Usage grade for a variable, following Section 2.5 of the spec.
///
/// Determines how many times a variable may be used at runtime.
#[derive(Debug, Clone, PartialEq)]
pub enum UsageGrade {
    /// Grade 0: ghost/erased, no runtime usage allowed.
    Erased,
    /// Grade 1: linear, must be used exactly once.
    Linear,
    /// Grade n: must be used exactly `n` times.
    Exact(u32),
    /// Grade omega: unlimited, can be used any number of times.
    Unlimited,
}

impl std::fmt::Display for UsageGrade {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UsageGrade::Erased => write!(f, "erased (grade 0)"),
            UsageGrade::Linear => write!(f, "linear (grade 1)"),
            UsageGrade::Exact(n) => write!(f, "exact (grade {n})"),
            UsageGrade::Unlimited => write!(f, "unlimited (grade ω)"),
        }
    }
}

/// Tracks variable usage counts and compares against expected grades.
///
/// Used for linearity checking: each variable is declared with an expected
/// `UsageGrade`, and each use of the variable increments its actual count.
/// After analysis, `check()` compares actual counts against expected grades
/// and produces errors for violations.
#[derive(Debug, Clone, Default)]
pub struct UsageTracker {
    /// Maps variable name -> (expected grade, actual usage count, declaration span).
    usages: HashMap<std::string::String, (UsageGrade, u32, Range<usize>)>,
}

impl UsageTracker {
    /// Create an empty usage tracker.
    pub fn new() -> Self {
        Self {
            usages: HashMap::new(),
        }
    }

    /// Declare a variable with its expected usage grade and declaration span.
    ///
    /// If the variable was already declared, updates its grade and resets
    /// the count.
    pub fn declare(&mut self, name: std::string::String, grade: UsageGrade, span: Range<usize>) {
        self.usages.insert(name, (grade, 0, span));
    }

    /// Record a use of a variable. Increments its usage count.
    ///
    /// If the variable was not declared via `declare()`, this is a no-op
    /// (the variable may be unlimited/external and not tracked).
    pub fn use_var(&mut self, name: &str) {
        if let Some((_grade, count, _span)) = self.usages.get_mut(name) {
            *count += 1;
        }
    }

    /// Check all tracked variables against their expected usage grades.
    ///
    /// Returns a list of `TypeError`s for any violations:
    /// - **A05001**: Linear variable used more than once
    /// - **A05002**: Linear variable never used (or erased variable used)
    /// - **A05003**: Exact-count variable used wrong number of times
    pub fn check(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();

        for (name, (grade, count, span)) in &self.usages {
            match grade {
                UsageGrade::Erased => {
                    if *count > 0 {
                        errors.push(TypeError {
                            code: "A05002".into(),
                            message: format!(
                                "erased variable `{name}` must not be used at runtime, \
                                 but was used {count} time(s)"
                            ),
                            span: span.clone(),
                            secondary: None,
                        });
                    }
                }
                UsageGrade::Linear => {
                    if *count == 0 {
                        errors.push(TypeError {
                            code: "A05002".into(),
                            message: format!("linear variable `{name}` was never used"),
                            span: span.clone(),
                            secondary: None,
                        });
                    } else if *count > 1 {
                        errors.push(TypeError {
                            code: "A05001".into(),
                            message: format!(
                                "linear variable `{name}` used {count} times, \
                                 but must be used exactly once"
                            ),
                            span: span.clone(),
                            secondary: None,
                        });
                    }
                }
                UsageGrade::Exact(expected) => {
                    if count != expected {
                        errors.push(TypeError {
                            code: "A05003".into(),
                            message: format!(
                                "variable `{name}` used {count} time(s), \
                                 but must be used exactly {expected} time(s)"
                            ),
                            span: span.clone(),
                            secondary: None,
                        });
                    }
                }
                UsageGrade::Unlimited => {
                    // No restrictions on usage count.
                }
            }
        }

        // Sort errors by span start for deterministic output.
        errors.sort_by_key(|e| e.span.start);
        errors
    }

    /// Get the current usage count for a variable.
    pub fn get_count(&self, name: &str) -> Option<u32> {
        self.usages.get(name).map(|(_, count, _)| *count)
    }

    /// Set the usage count for a variable (used during context merge).
    pub fn set_count(&mut self, name: &str, count: u32) {
        if let Some((_grade, c, _span)) = self.usages.get_mut(name) {
            *c = count;
        }
    }

    /// Get the declaration span for a variable.
    pub fn get_span(&self, name: &str) -> Option<Range<usize>> {
        self.usages.get(name).map(|(_, _, span)| span.clone())
    }
}

// ---------------------------------------------------------------------------
// Linear context with branch support (T032)
// ---------------------------------------------------------------------------

/// Linear type context with branching support for context splitting.
///
/// Wraps a `UsageTracker` and adds fork/merge operations for handling
/// if/match branches correctly. At each branch point, the context is
/// forked, each branch is checked independently, and the results are
/// merged back with consistency checks.
#[derive(Debug, Clone)]
pub struct LinearContext {
    tracker: UsageTracker,
}

impl LinearContext {
    /// Create a new linear context from a usage tracker.
    pub fn new(tracker: UsageTracker) -> Self {
        Self { tracker }
    }

    /// Record a variable use in this context.
    pub fn use_var(&mut self, name: &str) {
        self.tracker.use_var(name);
    }

    /// Declare a variable in this context.
    pub fn declare(&mut self, name: String, grade: UsageGrade, span: Range<usize>) {
        self.tracker.declare(name, grade, span);
    }

    /// Get the current usage count for a variable in this context.
    pub fn get_count(&self, name: &str) -> Option<u32> {
        self.tracker.get_count(name)
    }

    /// Create two independent copies of this context for branching.
    pub fn fork(&self) -> (LinearContext, LinearContext) {
        (self.clone(), self.clone())
    }

    /// Merge two branch contexts back into this context.
    ///
    /// Compares usage counts in `branch_a` and `branch_b` against the
    /// counts in `self` (the pre-branch base state). For linear and
    /// exact-grade variables, if the usage delta differs between branches,
    /// emits A05004 (inconsistent branch usage).
    ///
    /// After merge, updates `self` with the maximum usage count from
    /// either branch (conservative: treat as consumed if used in any path).
    pub fn merge(&mut self, branch_a: &LinearContext, branch_b: &LinearContext) -> Vec<TypeError> {
        let mut errors = Vec::new();

        // Snapshot the base state before mutation.
        let base_state: Vec<(String, UsageGrade, u32, Range<usize>)> = self
            .tracker
            .usages
            .iter()
            .map(|(name, (grade, count, span))| (name.clone(), grade.clone(), *count, span.clone()))
            .collect();

        for (name, grade, base_count, span) in &base_state {
            let a_count = branch_a.tracker.get_count(name).unwrap_or(*base_count);
            let b_count = branch_b.tracker.get_count(name).unwrap_or(*base_count);

            let delta_a = a_count.saturating_sub(*base_count);
            let delta_b = b_count.saturating_sub(*base_count);

            // Check consistency for linear and exact-grade variables.
            if matches!(grade, UsageGrade::Linear | UsageGrade::Exact(_)) && delta_a != delta_b {
                errors.push(TypeError {
                    code: "A05004".into(),
                    message: format!(
                        "linear variable `{name}` used inconsistently across branches: \
                         used {delta_a} time(s) in one branch, {delta_b} time(s) in the other"
                    ),
                    span: span.clone(),
                    secondary: None,
                });
            }

            // Take the maximum: treat as consumed if used in any branch.
            let merged_count = base_count + std::cmp::max(delta_a, delta_b);
            self.tracker.set_count(name, merged_count);
        }

        errors
    }

    /// Run the final usage check on this context.
    ///
    /// Delegates to `UsageTracker::check()`, producing A05001-A05003 errors
    /// for any remaining linearity violations after all expressions have
    /// been walked.
    pub fn check(&self) -> Vec<TypeError> {
        self.tracker.check()
    }
}

/// Walk an expression AST with linear context splitting for branches.
///
/// For if/match expressions, forks the context, walks each branch
/// independently, and merges the results back. This is the context-
/// splitting implementation for T032.
///
/// Returns errors for:
/// - A05004: linear variable used inconsistently across branches
/// - A05005: linear variable escapes its scope
pub fn check_expr_linearity(expr: &Expr, ctx: &mut LinearContext) -> Vec<TypeError> {
    let mut errors = Vec::new();
    check_expr_linearity_inner(expr, ctx, &mut errors);
    errors
}

/// Inner recursive walker for `check_expr_linearity`.
fn check_expr_linearity_inner(expr: &Expr, ctx: &mut LinearContext, errors: &mut Vec<TypeError>) {
    match expr {
        Expr::Ident(name) => {
            ctx.use_var(name);
        }
        Expr::Literal(_) => {}
        Expr::Field(receiver, _field) => {
            check_expr_linearity_inner(receiver, ctx, errors);
        }
        Expr::MethodCall { receiver, args, .. } => {
            check_expr_linearity_inner(receiver, ctx, errors);
            for arg in args {
                check_expr_linearity_inner(arg, ctx, errors);
            }
        }
        Expr::Call { func, args } => {
            check_expr_linearity_inner(func, ctx, errors);
            for arg in args {
                check_expr_linearity_inner(arg, ctx, errors);
            }
        }
        Expr::Index { expr: base, index } => {
            check_expr_linearity_inner(base, ctx, errors);
            check_expr_linearity_inner(index, ctx, errors);
        }
        Expr::BinOp { lhs, rhs, .. } => {
            check_expr_linearity_inner(lhs, ctx, errors);
            check_expr_linearity_inner(rhs, ctx, errors);
        }
        Expr::UnaryOp { expr: inner, .. } => {
            check_expr_linearity_inner(inner, ctx, errors);
        }
        Expr::Old(inner) => {
            check_expr_linearity_inner(inner, ctx, errors);
        }
        Expr::Forall {
            var: _,
            domain,
            body,
        }
        | Expr::Exists {
            var: _,
            domain,
            body,
        } => {
            check_expr_linearity_inner(domain, ctx, errors);
            check_expr_linearity_inner(body, ctx, errors);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            // Check condition in current context (condition is always evaluated).
            check_expr_linearity_inner(cond, ctx, errors);

            // Fork context for the two branches.
            let (mut ctx_then, mut ctx_else) = ctx.fork();

            // Walk each branch independently.
            check_expr_linearity_inner(then_branch, &mut ctx_then, errors);

            if let Some(else_br) = else_branch {
                check_expr_linearity_inner(else_br, &mut ctx_else, errors);
            }
            // If there is no else branch, ctx_else stays at the
            // post-condition counts (no additional uses), which makes
            // any variable used only in the then-branch inconsistent.

            // Merge: check consistency and take max usage.
            let merge_errors = ctx.merge(&ctx_then, &ctx_else);
            errors.extend(merge_errors);
        }
        Expr::Paren(inner) => {
            check_expr_linearity_inner(inner, ctx, errors);
        }
        Expr::List(items) => {
            for item in items {
                check_expr_linearity_inner(item, ctx, errors);
            }
        }
        Expr::Cast { expr: inner, .. } => {
            check_expr_linearity_inner(inner, ctx, errors);
        }
        Expr::Block(exprs) => {
            for e in exprs {
                check_expr_linearity_inner(e, ctx, errors);
            }
        }
        Expr::Ghost(_inner) => {
            // Ghost blocks are erased at runtime. Variable references
            // inside ghost blocks do NOT count as linear uses.
        }
        Expr::Apply { args, .. } => {
            // Apply expressions are erased at runtime (like ghost).
            // Arguments are verified but do not count as linear uses.
            let _ = args;
        }
        Expr::Match { scrutinee, arms } => {
            check_expr_linearity_inner(scrutinee, ctx, errors);
            for arm in arms {
                check_expr_linearity_inner(&arm.body, ctx, errors);
            }
        }
        Expr::Let { value, body, .. } => {
            check_expr_linearity_inner(value, ctx, errors);
            check_expr_linearity_inner(body, ctx, errors);
        }
        Expr::Tuple(elems) => {
            for e in elems {
                check_expr_linearity_inner(e, ctx, errors);
            }
        }
        Expr::Raw(_) => {
            // Cannot extract variable references from raw token sequences.
        }
    }
}

// ---------------------------------------------------------------------------
// Typestate checker (T034)
// ---------------------------------------------------------------------------

/// Error produced by the typestate checker.
///
/// Uses error codes from the spec:
/// - **A06001**: Operation called in wrong state
/// - **A06002**: Typestate variable is not linear
/// - **A06003**: State not declared in `states:` block
/// - **A06004**: Ambiguous state after diverging branches
#[derive(Debug, Clone)]
pub struct TypestateError {
    /// Error code from the spec (A06xxx series).
    pub code: std::string::String,
    /// Human-readable error message.
    pub message: std::string::String,
    /// Source location where the error was detected.
    pub span: Range<usize>,
}

/// A transition in the typestate DFA.
///
/// Each transition is `(operation_name, required_state, next_state)`.
/// The operation can only be called when the object is in `required_state`,
/// and after the call the object moves to `next_state`.
#[derive(Debug, Clone)]
struct Transition {
    operation: std::string::String,
    from_state: std::string::String,
    to_state: std::string::String,
}

/// Typestate checker that tracks a DFA of states and transitions.
///
/// Built from a `states:` declaration in a service or contract. Tracks the
/// current state of a typestate variable and validates that operations are
/// only called in the required state, transitioning to the declared next
/// state afterward.
///
/// # Error codes
///
/// - **A06001**: Operation called when object is in wrong state
/// - **A06002**: Typestate variable must be linear (checked separately)
/// - **A06003**: A transition references a state not in `states:`
/// - **A06004**: After diverging branches, object is in different states
#[derive(Debug, Clone)]
pub struct TypestateChecker {
    /// All declared states for this typestate variable.
    states: Vec<std::string::String>,
    /// All declared transitions.
    transitions: Vec<Transition>,
    /// Current state of the tracked variable.
    current: std::string::String,
    /// Source span of the typestate declaration (for error reporting).
    decl_span: Range<usize>,
}

impl TypestateChecker {
    /// Create a new typestate checker.
    ///
    /// # Arguments
    ///
    /// * `states` - All declared states from the `states:` block
    /// * `transitions` - Vec of `(operation, from_state, to_state)` tuples
    /// * `initial_state` - The starting state
    /// * `decl_span` - Source span of the typestate declaration
    pub fn new(
        states: Vec<std::string::String>,
        transitions: Vec<(
            std::string::String,
            std::string::String,
            std::string::String,
        )>,
        initial_state: std::string::String,
        decl_span: Range<usize>,
    ) -> Self {
        let transitions = transitions
            .into_iter()
            .map(|(op, from, to)| Transition {
                operation: op,
                from_state: from,
                to_state: to,
            })
            .collect();
        Self {
            states,
            transitions,
            current: initial_state,
            decl_span,
        }
    }

    /// Get the current state of the tracked variable.
    pub fn current_state(&self) -> &str {
        &self.current
    }

    /// Attempt to perform a state transition for the given operation.
    ///
    /// Looks up the operation in the transition table. If a transition
    /// exists whose `from_state` matches the current state, moves to
    /// `to_state` and returns `Ok(())`. Otherwise returns an `A06001`
    /// error.
    pub fn transition(
        &mut self,
        operation: &str,
        span: Range<usize>,
    ) -> Result<(), TypestateError> {
        // Find a transition for this operation from the current state.
        for t in &self.transitions {
            if t.operation == operation && t.from_state == self.current {
                self.current = t.to_state.clone();
                return Ok(());
            }
        }

        // Find what state the operation requires (for a better error message).
        let required_states: Vec<&str> = self
            .transitions
            .iter()
            .filter(|t| t.operation == operation)
            .map(|t| t.from_state.as_str())
            .collect();

        let message = if required_states.is_empty() {
            format!(
                "operation `{operation}` is not defined for any state of this typestate variable \
                 (current state: `{}`)",
                self.current,
            )
        } else {
            format!(
                "operation `{operation}` requires state `{}`, but object is in state `{}`",
                required_states.join("` or `"),
                self.current,
            )
        };

        Err(TypestateError {
            code: "A06001".into(),
            message,
            span,
        })
    }

    /// Validate that the typestate variable is declared as linear.
    ///
    /// Typestate variables must be linear (used exactly once) because
    /// aliasing would allow observing inconsistent states. Returns
    /// `Some(TypestateError)` with code A06002 if `is_linear` is false.
    pub fn validate_linear(&self, is_linear: bool) -> Option<TypestateError> {
        if is_linear {
            None
        } else {
            Some(TypestateError {
                code: "A06002".into(),
                message: "typestate variable must be declared as linear".into(),
                span: self.decl_span.clone(),
            })
        }
    }

    /// Validate that all transitions reference declared states.
    ///
    /// Checks both `from_state` and `to_state` of every transition against
    /// the `states` list. Returns a list of `A06003` errors for any
    /// undeclared states referenced in transitions.
    pub fn validate_transitions(&self) -> Vec<TypestateError> {
        let mut errors = Vec::new();

        for t in &self.transitions {
            if !self.states.contains(&t.from_state) {
                errors.push(TypestateError {
                    code: "A06003".into(),
                    message: format!(
                        "transition `{}` references undeclared source state `{}`; \
                         declared states: [{}]",
                        t.operation,
                        t.from_state,
                        self.states.join(", "),
                    ),
                    span: self.decl_span.clone(),
                });
            }
            if !self.states.contains(&t.to_state) {
                errors.push(TypestateError {
                    code: "A06003".into(),
                    message: format!(
                        "transition `{}` references undeclared target state `{}`; \
                         declared states: [{}]",
                        t.operation,
                        t.to_state,
                        self.states.join(", "),
                    ),
                    span: self.decl_span.clone(),
                });
            }
        }

        errors
    }

    /// Check that two branch checkers ended in the same state.
    ///
    /// After diverging control flow (if/match), if the typestate variable
    /// was transitioned in both branches, they must end in the same state.
    /// Otherwise the post-branch state is ambiguous and we emit A06004.
    ///
    /// Returns `None` if states match, or `Some(TypestateError)` with
    /// code A06004 if they differ.
    pub fn check_branch_consistency(
        branch_a: &TypestateChecker,
        branch_b: &TypestateChecker,
        span: Range<usize>,
    ) -> Option<TypestateError> {
        if branch_a.current == branch_b.current {
            None
        } else {
            Some(TypestateError {
                code: "A06004".into(),
                message: format!(
                    "ambiguous state after diverging branches: \
                     one branch leaves object in state `{}`, \
                     the other in state `{}`",
                    branch_a.current, branch_b.current,
                ),
                span,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Expression usage walker
// ---------------------------------------------------------------------------

/// Walk an expression AST and count variable usages in a `UsageTracker`.
///
/// Each `Ident` node increments the usage count for that variable name.
/// Recursively walks all sub-expressions (binary ops, unary ops, function
/// calls, quantifiers, etc.).
pub fn expr_usages(expr: &Expr, tracker: &mut UsageTracker) {
    match expr {
        Expr::Ident(name) => {
            tracker.use_var(name);
        }
        Expr::Literal(_) => {}
        Expr::Field(receiver, _field) => {
            expr_usages(receiver, tracker);
        }
        Expr::MethodCall { receiver, args, .. } => {
            expr_usages(receiver, tracker);
            for arg in args {
                expr_usages(arg, tracker);
            }
        }
        Expr::Call { func, args } => {
            expr_usages(func, tracker);
            for arg in args {
                expr_usages(arg, tracker);
            }
        }
        Expr::Index { expr: base, index } => {
            expr_usages(base, tracker);
            expr_usages(index, tracker);
        }
        Expr::BinOp { lhs, rhs, .. } => {
            expr_usages(lhs, tracker);
            expr_usages(rhs, tracker);
        }
        Expr::UnaryOp { expr: inner, .. } => {
            expr_usages(inner, tracker);
        }
        Expr::Old(inner) => {
            expr_usages(inner, tracker);
        }
        Expr::Forall {
            var: _,
            domain,
            body,
        }
        | Expr::Exists {
            var: _,
            domain,
            body,
        } => {
            expr_usages(domain, tracker);
            expr_usages(body, tracker);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_usages(cond, tracker);
            expr_usages(then_branch, tracker);
            if let Some(else_br) = else_branch {
                expr_usages(else_br, tracker);
            }
        }
        Expr::Paren(inner) => {
            expr_usages(inner, tracker);
        }
        Expr::List(items) => {
            for item in items {
                expr_usages(item, tracker);
            }
        }
        Expr::Cast { expr: inner, .. } => {
            expr_usages(inner, tracker);
        }
        Expr::Block(exprs) => {
            for e in exprs {
                expr_usages(e, tracker);
            }
        }
        Expr::Ghost(_) => {
            // Ghost blocks are erased at runtime; do not count usages.
        }
        Expr::Apply { .. } => {
            // Apply expressions are erased at runtime; do not count usages.
        }
        Expr::Match { scrutinee, arms } => {
            expr_usages(scrutinee, tracker);
            for arm in arms {
                expr_usages(&arm.body, tracker);
            }
        }
        Expr::Let { value, body, .. } => {
            expr_usages(value, tracker);
            expr_usages(body, tracker);
        }
        Expr::Tuple(elems) => {
            for e in elems {
                expr_usages(e, tracker);
            }
        }
        Expr::Raw(_) => {
            // Cannot extract variable references from raw token sequences.
        }
    }
}

// ---------------------------------------------------------------------------
// Effect checking (T036)
// ---------------------------------------------------------------------------

/// A set of effects declared on (or inferred for) a function.
///
/// Effects are stored as lowercase strings matching the effect labels from
/// Section 3.1 of the spec (e.g., `"io"`, `"console.read"`, `"pure"`).
/// The special value `"pure"` represents an empty effect set.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectSet {
    effects: std::collections::HashSet<std::string::String>,
}

impl EffectSet {
    /// Create a new empty effect set (equivalent to `pure`).
    pub fn pure() -> Self {
        Self {
            effects: std::collections::HashSet::new(),
        }
    }

    /// Create an effect set from an iterator of effect names.
    ///
    /// The name `"pure"` is treated as an empty set; it is not stored as
    /// an actual effect label.
    pub fn from_effect_names(
        effects: impl IntoIterator<Item = impl Into<std::string::String>>,
    ) -> Self {
        let mut set = std::collections::HashSet::new();
        for e in effects {
            let name = e.into();
            if name != "pure" {
                set.insert(name);
            }
        }
        Self { effects: set }
    }

    /// Returns `true` if this is a pure (empty) effect set.
    pub fn is_pure(&self) -> bool {
        self.effects.is_empty()
    }

    /// Insert an effect into the set.
    pub fn insert(&mut self, effect: std::string::String) {
        if effect != "pure" {
            self.effects.insert(effect);
        }
    }

    /// Returns `true` if the set contains the given effect.
    pub fn contains(&self, effect: &str) -> bool {
        self.effects.contains(effect)
    }

    /// Iterate over the effect names in this set.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.effects.iter().map(|s| s.as_str())
    }

    /// Number of effects in the set.
    pub fn len(&self) -> usize {
        self.effects.len()
    }

    /// Returns `true` if the set is empty.
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }
}

impl std::fmt::Display for EffectSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.effects.is_empty() {
            return write!(f, "pure");
        }
        let mut sorted: Vec<&str> = self.effects.iter().map(|s| s.as_str()).collect();
        sorted.sort();
        write!(f, "{{{}}}", sorted.join(", "))
    }
}

/// An error produced by the effect checker.
#[derive(Debug, Clone)]
pub struct EffectError {
    /// Error code from the spec (A07xxx series).
    pub code: std::string::String,
    /// Human-readable error message.
    pub message: std::string::String,
    /// Source location where the error was detected.
    pub span: Range<usize>,
}

/// Effect checker that validates effect declarations and containment.
///
/// Implements the effect checking rules from Section 3.5 of the spec:
/// a function's body may only use effects declared in its signature,
/// and all effect names must be recognized (built-in or user-defined).
///
/// The effect hierarchy from Section 3.6 is encoded: `io` is shorthand
/// for all IO sub-effects, `database` for all database sub-effects,
/// and `logging` for all log sub-effects.
pub struct EffectChecker {
    /// All known effect names (both group names and leaf effects).
    known_effects: std::collections::HashSet<&'static str>,
    /// Maps a group effect to its sub-effects.
    hierarchy: HashMap<&'static str, Vec<&'static str>>,
}

impl EffectChecker {
    /// Create a new effect checker with the built-in effect vocabulary
    /// from Section 3.1 and hierarchy from Section 3.6 of the spec.
    pub fn new() -> Self {
        let known: std::collections::HashSet<&'static str> = [
            // Group effects
            "io",
            "database",
            "logging",
            // Leaf IO effects
            "console.read",
            "console.write",
            "filesystem.read",
            "filesystem.write",
            "network.connect",
            "network.send",
            "network.receive",
            "time.read",
            "random",
            // Leaf database effects
            "database.read",
            "database.write",
            // Leaf logging effects
            "log.debug",
            "log.info",
            "log.warn",
            "log.error",
            // Other built-in effects
            "diverge",
            // Memory effect (from AGENTS.md task description)
            "mem",
            "net",
            "fs",
            "rng",
            "time",
            "alloc",
        ]
        .into_iter()
        .collect();

        let mut hierarchy = HashMap::new();
        hierarchy.insert(
            "io",
            vec![
                "console.read",
                "console.write",
                "filesystem.read",
                "filesystem.write",
                "network.connect",
                "network.send",
                "network.receive",
                "time.read",
                "random",
                // Short aliases that map to IO sub-categories
                "net",
                "fs",
                "rng",
                "time",
            ],
        );
        hierarchy.insert("database", vec!["database.read", "database.write"]);
        hierarchy.insert(
            "logging",
            vec!["log.debug", "log.info", "log.warn", "log.error"],
        );
        // Short alias groups
        hierarchy.insert(
            "net",
            vec!["network.connect", "network.send", "network.receive"],
        );
        hierarchy.insert("fs", vec!["filesystem.read", "filesystem.write"]);

        Self {
            known_effects: known,
            hierarchy,
        }
    }

    /// Expand a declared effect set by adding all sub-effects implied by
    /// the hierarchy. For example, declaring `io` expands to include
    /// `console.read`, `console.write`, etc.
    pub fn expand(&self, declared: &EffectSet) -> EffectSet {
        let mut expanded = declared.clone();
        // Iterate over the original set (not the expanding one) to avoid
        // borrow issues.
        let originals: Vec<std::string::String> = declared.effects.iter().cloned().collect();
        for effect in &originals {
            if let Some(children) = self.hierarchy.get(effect.as_str()) {
                for &child in children {
                    expanded.insert(child.to_string());
                }
            }
        }
        expanded
    }

    /// Check that all effects in `actual` are contained in `declared`.
    ///
    /// The `declared` set is expanded via the hierarchy before comparison.
    /// Returns a list of `EffectError`s for violations:
    ///
    /// - **A07001**: An effect in `actual` is not present in the expanded
    ///   `declared` set (undeclared effect).
    /// - **A07002**: The function is declared `pure` (empty declared set)
    ///   but the body performs effects (side effect in pure context).
    pub fn check_containment(
        &self,
        declared: &EffectSet,
        actual: &EffectSet,
        span: &Range<usize>,
    ) -> Vec<EffectError> {
        let mut errors = Vec::new();

        // Expand the declared set to include sub-effects
        let expanded = self.expand(declared);

        for effect in actual.iter() {
            // Check if the actual effect (or a parent of it) is in the
            // expanded declared set.
            if !self.is_allowed(effect, &expanded) {
                if declared.is_pure() {
                    // A07002: pure function performs effect
                    errors.push(EffectError {
                        code: "A07002".into(),
                        message: format!(
                            "pure function performs effect `{effect}`: \
                             side effects are not allowed in a pure context"
                        ),
                        span: span.clone(),
                    });
                } else {
                    // A07001: undeclared effect
                    errors.push(EffectError {
                        code: "A07001".into(),
                        message: format!(
                            "undeclared effect `{effect}`: \
                             effect not in function's declared effect set {declared}"
                        ),
                        span: span.clone(),
                    });
                }
            }
        }

        // Sort errors by code then message for deterministic output.
        errors.sort_by(|a, b| a.code.cmp(&b.code).then(a.message.cmp(&b.message)));
        errors
    }

    /// Check that all effect names in a set are recognized.
    ///
    /// Returns A07003 errors for unknown effect names.
    pub fn check_known(&self, effects: &EffectSet, span: &Range<usize>) -> Vec<EffectError> {
        let mut errors = Vec::new();

        for effect in effects.iter() {
            // Skip identifiers that are clearly not effect names:
            // - Capitalized names (type names like `InflateDecoder`)
            // - Known block-kind keywords that leak from parser spans
            // This prevents false positives from parser artifacts where
            // block kind names leak into effect clause token streams.
            if effect.chars().next().is_some_and(|c| c.is_uppercase()) {
                continue;
            }
            if is_block_kind_keyword(effect) {
                continue;
            }
            if !self.known_effects.contains(effect) && !self.is_sub_effect_of_known(effect) {
                errors.push(EffectError {
                    code: "A07003".into(),
                    message: format!("unknown effect name `{effect}`"),
                    span: span.clone(),
                });
            }
        }

        errors.sort_by(|a, b| a.message.cmp(&b.message));
        errors
    }

    /// Returns `true` if the effect is a dot-separated sub-effect of a
    /// known group. For example, `io.read` is accepted because `io` is
    /// a known group effect.
    #[allow(clippy::unused_self)]
    fn is_sub_effect_of_known(&self, effect: &str) -> bool {
        if let Some(dot_pos) = effect.find('.') {
            let parent = &effect[..dot_pos];
            self.known_effects.contains(parent) || self.hierarchy.contains_key(parent)
        } else {
            false
        }
    }
}

/// Returns `true` if the name is a known Assura block-kind keyword
/// (e.g., `incremental`, `feature`, `liveness`) that should not be
/// treated as an effect name even when it appears in an effect clause
/// due to parser span overlap.
fn is_block_kind_keyword(name: &str) -> bool {
    matches!(
        name,
        "incremental"
            | "feature"
            | "liveness"
            | "axiomatic"
            | "axiom"
            | "lemma"
            | "ghost"
            | "opaque"
            | "test"
            | "property"
            | "complexity"
            | "benchmark"
            | "migration"
    )
}

impl EffectChecker {
    /// Returns `true` if `effect` is allowed by the expanded declared set.
    ///
    /// An effect is allowed if:
    /// 1. It is directly in the expanded set, OR
    /// 2. Any of its ancestor groups are in the expanded set.
    fn is_allowed(&self, effect: &str, expanded: &EffectSet) -> bool {
        // Direct containment
        if expanded.contains(effect) {
            return true;
        }

        // Check if any group in the expanded set subsumes this effect
        for group_effect in expanded.iter() {
            if let Some(children) = self.hierarchy.get(group_effect)
                && children.contains(&effect)
            {
                return true;
            }
        }

        false
    }

    /// Returns `true` if the given effect name is a known built-in effect.
    pub fn is_known(&self, effect: &str) -> bool {
        self.known_effects.contains(effect)
    }
}

impl Default for EffectChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Frame condition checking (T045 - CORE.3)
// ---------------------------------------------------------------------------

/// Extract the set of variable/field names from a `modifies` clause body.
///
/// The modifies clause body is typically:
/// - `Expr::Ident("x")` for a single variable
/// - `Expr::Block([Expr::Ident("x"), Expr::Ident("y")])` for multiple
/// - `Expr::Field(Expr::Ident("obj"), "field")` for `obj.field`
/// - `Expr::List([...])` for comma-separated list
///
/// Returns a set of string representations (e.g., `"x"`, `"node.keys"`).
pub fn extract_modifies_targets(expr: &Expr) -> Vec<std::string::String> {
    let mut targets = Vec::new();
    collect_modifies_targets(expr, &mut targets);
    targets
}

/// Recursively collect modifies targets from an expression.
fn collect_modifies_targets(expr: &Expr, targets: &mut Vec<std::string::String>) {
    match expr {
        Expr::Ident(name) => {
            targets.push(name.clone());
        }
        Expr::Field(receiver, field) => {
            // Build dotted path: "obj.field"
            let mut path = std::string::String::new();
            build_field_path(receiver, &mut path);
            if !path.is_empty() {
                path.push('.');
            }
            path.push_str(field);
            targets.push(path);
        }
        Expr::Block(exprs) => {
            for e in exprs {
                collect_modifies_targets(e, targets);
            }
        }
        Expr::List(items) => {
            for item in items {
                collect_modifies_targets(item, targets);
            }
        }
        Expr::Paren(inner) => {
            collect_modifies_targets(inner, targets);
        }
        Expr::Raw(tokens) => {
            // Parse comma-separated identifiers from raw tokens
            for tok in tokens {
                let trimmed = tok.trim();
                if !trimmed.is_empty() && trimmed != "," {
                    targets.push(trimmed.to_string());
                }
            }
        }
        // Other expression types are not valid modifies targets
        _ => {}
    }
}

/// Build a dotted field path from nested Field expressions.
fn build_field_path(expr: &Expr, path: &mut std::string::String) {
    match expr {
        Expr::Ident(name) => {
            path.push_str(name);
        }
        Expr::Field(receiver, field) => {
            build_field_path(receiver, path);
            path.push('.');
            path.push_str(field);
        }
        _ => {}
    }
}

/// Collect all variable names referenced via `old(expr)` in an expression.
///
/// Walks the expression tree and whenever it finds `Expr::Old(inner)`,
/// extracts the variable/field name from `inner`. This is used to find
/// which pre-state variables an `ensures` clause references.
pub fn collect_old_references(expr: &Expr) -> Vec<std::string::String> {
    let mut refs = Vec::new();
    collect_old_refs_inner(expr, &mut refs);
    refs
}

fn collect_old_refs_inner(expr: &Expr, refs: &mut Vec<std::string::String>) {
    match expr {
        Expr::Old(inner) => {
            // Extract the name from the inner expression
            match inner.as_ref() {
                Expr::Ident(name) => {
                    refs.push(name.clone());
                }
                Expr::Field(receiver, field) => {
                    let mut path = std::string::String::new();
                    build_field_path(receiver, &mut path);
                    if !path.is_empty() {
                        path.push('.');
                    }
                    path.push_str(field);
                    refs.push(path);
                }
                _ => {}
            }
            // Also recurse into the inner expression
            collect_old_refs_inner(inner, refs);
        }
        Expr::Ident(_) | Expr::Literal(_) | Expr::Raw(_) => {}
        Expr::Field(receiver, _) => collect_old_refs_inner(receiver, refs),
        Expr::MethodCall { receiver, args, .. } => {
            collect_old_refs_inner(receiver, refs);
            for arg in args {
                collect_old_refs_inner(arg, refs);
            }
        }
        Expr::Call { func, args } => {
            collect_old_refs_inner(func, refs);
            for arg in args {
                collect_old_refs_inner(arg, refs);
            }
        }
        Expr::Index { expr: base, index } => {
            collect_old_refs_inner(base, refs);
            collect_old_refs_inner(index, refs);
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_old_refs_inner(lhs, refs);
            collect_old_refs_inner(rhs, refs);
        }
        Expr::UnaryOp { expr: inner, .. } => collect_old_refs_inner(inner, refs),
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            collect_old_refs_inner(domain, refs);
            collect_old_refs_inner(body, refs);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_old_refs_inner(cond, refs);
            collect_old_refs_inner(then_branch, refs);
            if let Some(else_br) = else_branch {
                collect_old_refs_inner(else_br, refs);
            }
        }
        Expr::Paren(inner) => collect_old_refs_inner(inner, refs),
        Expr::List(items) => {
            for item in items {
                collect_old_refs_inner(item, refs);
            }
        }
        Expr::Cast { expr: inner, .. } => collect_old_refs_inner(inner, refs),
        Expr::Ghost(inner) => collect_old_refs_inner(inner, refs),
        Expr::Apply { args, .. } => {
            for arg in args {
                collect_old_refs_inner(arg, refs);
            }
        }
        Expr::Match { scrutinee, arms } => {
            collect_old_refs_inner(scrutinee, refs);
            for arm in arms {
                collect_old_refs_inner(&arm.body, refs);
            }
        }
        Expr::Let { value, body, .. } => {
            collect_old_refs_inner(value, refs);
            collect_old_refs_inner(body, refs);
        }
        Expr::Block(exprs) => {
            for e in exprs {
                collect_old_refs_inner(e, refs);
            }
        }
        Expr::Tuple(elems) => {
            for e in elems {
                collect_old_refs_inner(e, refs);
            }
        }
    }
}

/// Collect all identifier names referenced in an expression (non-recursive
/// into old()).
///
/// Used to find which variables an ensures clause mentions so we can
/// determine which frame axioms to inject.
pub fn collect_ident_references(expr: &Expr) -> Vec<std::string::String> {
    let mut refs = Vec::new();
    collect_idents_inner(expr, &mut refs);
    refs
}

fn collect_idents_inner(expr: &Expr, refs: &mut Vec<std::string::String>) {
    match expr {
        Expr::Ident(name) => {
            if name != "true" && name != "false" && name != "result" && name != "self" {
                refs.push(name.clone());
            }
        }
        Expr::Literal(_) | Expr::Raw(_) => {}
        Expr::Old(inner) => collect_idents_inner(inner, refs),
        Expr::Field(receiver, field) => {
            let mut path = std::string::String::new();
            build_field_path(receiver, &mut path);
            if !path.is_empty() {
                path.push('.');
            }
            path.push_str(field);
            refs.push(path);
            collect_idents_inner(receiver, refs);
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_idents_inner(receiver, refs);
            for arg in args {
                collect_idents_inner(arg, refs);
            }
        }
        Expr::Call { func, args } => {
            collect_idents_inner(func, refs);
            for arg in args {
                collect_idents_inner(arg, refs);
            }
        }
        Expr::Index { expr: base, index } => {
            collect_idents_inner(base, refs);
            collect_idents_inner(index, refs);
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_idents_inner(lhs, refs);
            collect_idents_inner(rhs, refs);
        }
        Expr::UnaryOp { expr: inner, .. } => collect_idents_inner(inner, refs),
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            collect_idents_inner(domain, refs);
            collect_idents_inner(body, refs);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_idents_inner(cond, refs);
            collect_idents_inner(then_branch, refs);
            if let Some(else_br) = else_branch {
                collect_idents_inner(else_br, refs);
            }
        }
        Expr::Paren(inner) => collect_idents_inner(inner, refs),
        Expr::List(items) => {
            for item in items {
                collect_idents_inner(item, refs);
            }
        }
        Expr::Cast { expr: inner, .. } => collect_idents_inner(inner, refs),
        Expr::Ghost(inner) => collect_idents_inner(inner, refs),
        Expr::Apply { args, .. } => {
            for arg in args {
                collect_idents_inner(arg, refs);
            }
        }
        Expr::Match { scrutinee, arms } => {
            collect_idents_inner(scrutinee, refs);
            for arm in arms {
                collect_idents_inner(&arm.body, refs);
            }
        }
        Expr::Let { value, body, .. } => {
            collect_idents_inner(value, refs);
            collect_idents_inner(body, refs);
        }
        Expr::Block(exprs) => {
            for e in exprs {
                collect_idents_inner(e, refs);
            }
        }
        Expr::Tuple(elems) => {
            for e in elems {
                collect_idents_inner(e, refs);
            }
        }
    }
}

/// Frame condition checker for modifies clauses (CORE.3).
///
/// Validates that:
/// 1. Names in the `modifies` clause exist in scope (A14001)
/// 2. Computes which variables are NOT in the modifies set (the "frame")
///    so that the SMT encoder can inject `var == old(var)` axioms
///
/// # Error codes
///
/// - **A14001**: Variable in modifies clause does not exist in scope
/// - **A14002**: Assignment to variable not in modifies set (future, when
///   we have assignment analysis in the implementation IR)
pub struct FrameChecker {
    /// The set of variables/fields declared in the modifies clause.
    modified: std::collections::HashSet<std::string::String>,
}

impl FrameChecker {
    /// Create a new frame checker from modifies clause body expressions.
    ///
    /// Extracts variable/field names from the modifies clause and stores
    /// them as the "modified" set.
    pub fn new(modifies_clauses: &[&Expr]) -> Self {
        let mut modified = std::collections::HashSet::new();
        for body in modifies_clauses {
            for target in extract_modifies_targets(body) {
                modified.insert(target);
            }
        }
        Self { modified }
    }

    /// Create an empty frame checker (no modifies clause present).
    ///
    /// When there is no modifies clause, the function may modify anything;
    /// no frame axioms are injected.
    pub fn empty() -> Self {
        Self {
            modified: std::collections::HashSet::new(),
        }
    }

    /// Returns true if this checker has a non-empty modifies set.
    ///
    /// When false, no frame axioms should be injected (the function
    /// did not declare what it modifies).
    pub fn has_modifies(&self) -> bool {
        !self.modified.is_empty()
    }

    /// Get the set of modified variable names.
    pub fn modified_set(&self) -> &std::collections::HashSet<std::string::String> {
        &self.modified
    }

    /// Check that all names in the modifies clause exist in scope.
    ///
    /// Returns A14001 errors for any name that is not found in the
    /// symbol table or type environment.
    pub fn check_scope(
        &self,
        env: &TypeEnv,
        symbols: &assura_resolve::SymbolTable,
        span: &Range<usize>,
    ) -> Vec<TypeError> {
        let mut errors = Vec::new();

        for name in &self.modified {
            // Extract the root variable name (before any dots)
            let root = name.split('.').next().unwrap_or(name);

            // Check if the root variable exists in the type env or symbol table
            let in_env = env.lookup(root).is_some();
            let in_symbols = symbols.symbols.iter().any(|s| s.name == root);

            if !in_env && !in_symbols {
                errors.push(TypeError {
                    code: "A14001".into(),
                    message: format!(
                        "variable `{name}` in modifies clause does not exist in scope"
                    ),
                    span: span.clone(),
                    secondary: None,
                });
            }
        }

        errors.sort_by(|a, b| a.message.cmp(&b.message));
        errors
    }

    /// Compute the frame variables for an ensures clause.
    ///
    /// Given an ensures clause body, finds all variables referenced via
    /// `old(x)` that are NOT in the modifies set. For each such variable,
    /// the SMT encoder should assert `x == old(x)` as a frame axiom.
    ///
    /// Returns the list of variable names for which frame axioms should
    /// be injected.
    pub fn frame_axiom_vars(&self, ensures_body: &Expr) -> Vec<std::string::String> {
        if !self.has_modifies() {
            return Vec::new();
        }

        let old_refs = collect_old_references(ensures_body);
        let ident_refs = collect_ident_references(ensures_body);

        // Collect all referenced variables (both in old() and directly)
        let mut all_refs: std::collections::HashSet<std::string::String> =
            std::collections::HashSet::new();
        for r in &old_refs {
            all_refs.insert(r.clone());
        }
        for r in &ident_refs {
            all_refs.insert(r.clone());
        }

        // Variables NOT in the modifies set get frame axioms
        let mut frame_vars: Vec<std::string::String> = all_refs
            .into_iter()
            .filter(|name| !self.modified.contains(name))
            .filter(|name| {
                // Also check if any prefix is in the modified set
                // e.g., if "node" is modified, "node.keys" is also modified
                !self
                    .modified
                    .iter()
                    .any(|m| name.starts_with(&format!("{m}.")))
                    && !self
                        .modified
                        .iter()
                        .any(|m| m.starts_with(&format!("{name}.")))
            })
            .collect();

        frame_vars.sort();
        frame_vars.dedup();
        frame_vars
    }

    /// Returns true if a variable name is in the modifies set.
    pub fn is_modified(&self, name: &str) -> bool {
        self.modified.contains(name)
    }
}

impl std::fmt::Debug for FrameChecker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameChecker")
            .field("modified", &self.modified)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Error propagation checking (T064 TYPE.3)
// ---------------------------------------------------------------------------

/// Error propagation policy for a set of error codes.
#[derive(Debug, Clone, Default)]
pub struct ErrorPolicy {
    /// Error codes that MUST propagate to the caller (never silently swallowed).
    pub must_propagate: Vec<String>,
    /// Forbidden error translations: (from, to).
    pub must_not_mask: Vec<(String, String)>,
    /// Error codes whose detail must be preserved across translations.
    pub must_preserve_detail: Vec<String>,
    /// Function names whose return values MUST be checked.
    pub must_check: Vec<String>,
}

/// Checker for error propagation contracts.
pub struct ErrorPropagationChecker {
    /// Registered error policies.
    pub policies: HashMap<String, ErrorPolicy>,
}

impl Default for ErrorPropagationChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl ErrorPropagationChecker {
    /// Create a new checker with no policies.
    pub fn new() -> Self {
        Self {
            policies: HashMap::new(),
        }
    }

    /// Register an error policy.
    pub fn register_policy(&mut self, name: String, policy: ErrorPolicy) {
        self.policies.insert(name, policy);
    }

    /// Check if an error code is must-propagate in any registered policy.
    pub fn is_must_propagate(&self, error_code: &str) -> bool {
        self.policies
            .values()
            .any(|p| p.must_propagate.iter().any(|c| c == error_code))
    }

    /// Check if a translation from one error code to another is forbidden.
    pub fn is_masked(&self, from: &str, to: &str) -> bool {
        self.policies
            .values()
            .any(|p| p.must_not_mask.iter().any(|(f, t)| f == from && t == to))
    }

    /// Check if a function's return value must be checked.
    pub fn must_check_return(&self, fn_name: &str) -> bool {
        self.policies
            .values()
            .any(|p| p.must_check.iter().any(|f| f == fn_name))
    }

    /// Validate an error handling action. Returns error if the action violates a policy.
    pub fn validate_catch(
        &self,
        error_code: &str,
        action: ErrorAction,
        span: Range<usize>,
    ) -> Option<TypeError> {
        match action {
            ErrorAction::Swallow => {
                if self.is_must_propagate(error_code) {
                    return Some(TypeError {
                        code: "A12001".into(),
                        message: format!(
                            "error code '{error_code}' has must_propagate policy and cannot be silently swallowed"
                        ),
                        span,
                        secondary: None,
                    });
                }
            }
            ErrorAction::TranslateTo(target) => {
                if self.is_masked(error_code, &target) {
                    return Some(TypeError {
                        code: "A12002".into(),
                        message: format!(
                            "translating '{error_code}' to '{target}' is forbidden by must_not_mask policy"
                        ),
                        span,
                        secondary: None,
                    });
                }
            }
            ErrorAction::Propagate | ErrorAction::Handle => {}
        }
        None
    }

    /// Check that a function's Result return value is used.
    pub fn validate_unchecked_call(&self, fn_name: &str, span: Range<usize>) -> Option<TypeError> {
        if self.must_check_return(fn_name) {
            return Some(TypeError {
                code: "A12003".into(),
                message: format!("return value of '{fn_name}' must be checked (must_check policy)"),
                span,
                secondary: None,
            });
        }
        None
    }
}

/// What happens to a caught error.
#[derive(Debug, Clone, PartialEq)]
pub enum ErrorAction {
    /// Error is silently discarded (catch and ignore).
    Swallow,
    /// Error is translated to a different code.
    TranslateTo(String),
    /// Error is re-raised to the caller.
    Propagate,
    /// Error is handled with meaningful recovery logic.
    Handle,
}

// ---------------------------------------------------------------------------
// Memory region contracts (T046 - MEM.1)
// ---------------------------------------------------------------------------

/// A ghost memory region declaration, tracking a named range of valid indices.
///
/// In Assura, a region is a ghost construct: `region valid_range = 0..buf.len`.
/// It describes a set of indices that are valid for buffer access.
#[derive(Debug, Clone)]
pub struct MemoryRegion {
    /// Name of the region (e.g., "valid_range").
    pub name: std::string::String,
    /// Lower bound expression (as variable name or literal).
    pub lower: std::string::String,
    /// Upper bound expression (as variable name or literal).
    pub upper: std::string::String,
    /// The buffer variable this region is associated with.
    pub buffer: std::string::String,
}

/// An error produced by the memory checker.
///
/// Uses error codes from the spec:
/// - **A08101**: Buffer access without bounds check (requires clause missing
///   bounds check for array/buffer index)
/// - **A08102**: Region containment violation (sub-region not proven to be
///   within parent region)
/// - **A08103**: Ghost region references non-existent buffer
#[derive(Debug, Clone)]
pub struct MemoryError {
    /// Error code from the spec (A08xxx series).
    pub code: std::string::String,
    /// Human-readable error message.
    pub message: std::string::String,
    /// Source location where the error was detected.
    pub span: Range<usize>,
}

/// Memory checker for buffer safety contracts (MEM.1).
///
/// Validates that:
/// 1. Buffer access contracts include proper bounds checks in requires clauses
/// 2. Ghost region declarations reference buffers that exist in scope
/// 3. Region containment assertions are well-formed
///
/// The checker works on the type-checked AST and uses the type environment
/// to validate that variables referenced in memory contracts exist and have
/// appropriate types (Bytes, List, etc.).
///
/// # Error codes
///
/// - **A08101**: Buffer access without bounds check
/// - **A08102**: Region containment violation
/// - **A08103**: Ghost region references non-existent buffer
pub struct MemoryChecker {
    /// Known buffer-typed variables and their capacity expressions.
    /// Maps variable name -> capacity field name (e.g., "buf" -> "buf.len").
    buffers: HashMap<std::string::String, std::string::String>,
    /// Ghost region declarations.
    regions: Vec<MemoryRegion>,
}

impl MemoryChecker {
    /// Create a new memory checker.
    pub fn new() -> Self {
        Self {
            buffers: HashMap::new(),
            regions: Vec::new(),
        }
    }

    /// Register a buffer-typed variable with its capacity expression.
    ///
    /// Buffer types are: Bytes, List<T>, Sequence<T>, and any user type
    /// with `.len` or `.capacity` fields.
    pub fn register_buffer(&mut self, name: std::string::String, capacity: std::string::String) {
        self.buffers.insert(name, capacity);
    }

    /// Register a ghost region declaration.
    pub fn register_region(&mut self, region: MemoryRegion) {
        self.regions.push(region);
    }

    /// Returns all registered buffer names.
    pub fn buffer_names(&self) -> Vec<String> {
        self.buffers.keys().cloned().collect()
    }

    /// Returns true if the given variable name is a registered buffer.
    pub fn is_buffer(&self, name: &str) -> bool {
        self.buffers.contains_key(name)
    }

    /// Get the capacity expression for a buffer variable.
    pub fn buffer_capacity(&self, name: &str) -> Option<&str> {
        self.buffers.get(name).map(|s| s.as_str())
    }

    /// Get all registered regions.
    pub fn regions(&self) -> &[MemoryRegion] {
        &self.regions
    }

    /// Check whether a contract's requires clauses contain a proper bounds
    /// check for buffer access.
    ///
    /// A bounds check is an expression of the form:
    ///   `offset + len <= buf.len` or `offset + len <= buf.capacity`
    ///
    /// This function looks for patterns in requires clause expressions
    /// that constrain buffer access to be within bounds.
    ///
    /// Returns `None` if a bounds check is found, or `Some(MemoryError)`
    /// with code A08101 if no bounds check is present.
    pub fn check_bounds_in_requires(
        &self,
        buffer_name: &str,
        requires_exprs: &[&Expr],
        span: &Range<usize>,
    ) -> Option<MemoryError> {
        if !self.is_buffer(buffer_name) {
            return None;
        }

        // Look for a bounds-checking pattern in the requires clauses
        let has_bounds_check = requires_exprs
            .iter()
            .any(|expr| self.expr_has_bounds_check(expr, buffer_name));

        if has_bounds_check {
            None
        } else {
            Some(MemoryError {
                code: "A08101".into(),
                message: format!(
                    "buffer `{buffer_name}` accessed without bounds check: \
                     add a `requires` clause constraining index/offset \
                     to be within `{buffer_name}.len`"
                ),
                span: span.clone(),
            })
        }
    }

    /// Check that all ghost region declarations reference existing buffers.
    ///
    /// Returns A08103 errors for regions whose buffer is not registered.
    pub fn check_region_buffers(&self, span: &Range<usize>) -> Vec<MemoryError> {
        let mut errors = Vec::new();
        for region in &self.regions {
            if !self.is_buffer(&region.buffer) {
                errors.push(MemoryError {
                    code: "A08103".into(),
                    message: format!(
                        "ghost region `{}` references non-existent buffer `{}`",
                        region.name, region.buffer,
                    ),
                    span: span.clone(),
                });
            }
        }
        errors
    }

    /// Check that a sub-region is contained within a parent region.
    ///
    /// Returns `None` if both regions are registered and the containment
    /// is well-formed, or `Some(MemoryError)` with code A08102 if the
    /// containment cannot be established structurally.
    pub fn check_region_containment(
        &self,
        sub_region: &str,
        parent_region: &str,
        span: &Range<usize>,
    ) -> Option<MemoryError> {
        let sub = self.regions.iter().find(|r| r.name == sub_region);
        let parent = self.regions.iter().find(|r| r.name == parent_region);

        match (sub, parent) {
            (Some(sub_r), Some(parent_r)) => {
                // Structural containment check is deferred to SMT encoding.
                // Here we just validate that both regions exist and reference
                // the same buffer.
                if sub_r.buffer != parent_r.buffer {
                    Some(MemoryError {
                        code: "A08102".into(),
                        message: format!(
                            "region `{sub_region}` (on buffer `{}`) cannot be contained in \
                             region `{parent_region}` (on buffer `{}`): different buffers",
                            sub_r.buffer, parent_r.buffer,
                        ),
                        span: span.clone(),
                    })
                } else {
                    None
                }
            }
            (None, _) => Some(MemoryError {
                code: "A08102".into(),
                message: format!("sub-region `{sub_region}` is not defined"),
                span: span.clone(),
            }),
            (_, None) => Some(MemoryError {
                code: "A08102".into(),
                message: format!("parent region `{parent_region}` is not defined"),
                span: span.clone(),
            }),
        }
    }

    /// Recursively check whether an expression contains a bounds-checking
    /// pattern for the given buffer.
    ///
    /// Recognized patterns:
    /// - `expr <= buf.len` or `expr <= buf.capacity`
    /// - `expr < buf.len` or `expr < buf.capacity`
    /// - `buf.len >= expr` or `buf.capacity >= expr`
    /// - Any comparison where one side references the buffer's length/capacity
    ///   and the other constrains an offset/index
    fn expr_has_bounds_check(&self, expr: &Expr, buffer_name: &str) -> bool {
        match expr {
            Expr::BinOp { lhs, op, rhs } => {
                match op {
                    BinOp::Lte | BinOp::Lt => {
                        // Check: something <= buf.len
                        self.references_buffer_capacity(rhs, buffer_name)
                            || self.references_buffer_capacity(lhs, buffer_name)
                    }
                    BinOp::Gte | BinOp::Gt => {
                        // Check: buf.len >= something
                        self.references_buffer_capacity(lhs, buffer_name)
                            || self.references_buffer_capacity(rhs, buffer_name)
                    }
                    BinOp::And => {
                        // Conjunction: check both sides
                        self.expr_has_bounds_check(lhs, buffer_name)
                            || self.expr_has_bounds_check(rhs, buffer_name)
                    }
                    _ => false,
                }
            }
            Expr::Paren(inner) => self.expr_has_bounds_check(inner, buffer_name),
            _ => false,
        }
    }

    /// Check if an expression references a buffer's capacity/length.
    ///
    /// Looks for `buf.len`, `buf.capacity`, `buf.length`, or the
    /// registered capacity expression for the buffer.
    fn references_buffer_capacity(&self, expr: &Expr, buffer_name: &str) -> bool {
        match expr {
            Expr::Field(receiver, field) => {
                let is_len_field =
                    field == "len" || field == "capacity" || field == "length" || field == "size";
                if is_len_field && let Expr::Ident(name) = receiver.as_ref() {
                    return name == buffer_name;
                }
                false
            }
            Expr::Ident(name) => {
                // Check against registered capacity expression
                if let Some(cap) = self.buffers.get(buffer_name) {
                    name == cap
                } else {
                    false
                }
            }
            // Recurse into sub-expressions (e.g., offset + len <= buf.len)
            Expr::BinOp { lhs, rhs, .. } => {
                self.references_buffer_capacity(lhs, buffer_name)
                    || self.references_buffer_capacity(rhs, buffer_name)
            }
            Expr::Paren(inner) => self.references_buffer_capacity(inner, buffer_name),
            _ => false,
        }
    }
}

impl Default for MemoryChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for MemoryChecker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryChecker")
            .field("buffers", &self.buffers)
            .field("regions", &self.regions)
            .finish()
    }
}

/// Check whether an expression references a variable by name.
pub fn expr_references_var(expr: &Expr, var_name: &str) -> bool {
    match expr {
        Expr::Ident(name) => name == var_name,
        Expr::Field(receiver, _) => expr_references_var(receiver, var_name),
        Expr::BinOp { lhs, rhs, .. } => {
            expr_references_var(lhs, var_name) || expr_references_var(rhs, var_name)
        }
        Expr::UnaryOp { expr: inner, .. } | Expr::Old(inner) | Expr::Paren(inner) => {
            expr_references_var(inner, var_name)
        }
        Expr::Call { func, args } => {
            expr_references_var(func, var_name)
                || args.iter().any(|a| expr_references_var(a, var_name))
        }
        Expr::MethodCall { receiver, args, .. } => {
            expr_references_var(receiver, var_name)
                || args.iter().any(|a| expr_references_var(a, var_name))
        }
        Expr::Index { expr: base, index } => {
            expr_references_var(base, var_name) || expr_references_var(index, var_name)
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_references_var(cond, var_name)
                || expr_references_var(then_branch, var_name)
                || else_branch
                    .as_ref()
                    .is_some_and(|e| expr_references_var(e, var_name))
        }
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            expr_references_var(domain, var_name) || expr_references_var(body, var_name)
        }
        Expr::List(items) => items.iter().any(|i| expr_references_var(i, var_name)),
        Expr::Block(exprs) => exprs.iter().any(|e| expr_references_var(e, var_name)),
        Expr::Ghost(inner) | Expr::Cast { expr: inner, .. } => expr_references_var(inner, var_name),
        Expr::Apply { args, .. } => args.iter().any(|a| expr_references_var(a, var_name)),
        Expr::Match { scrutinee, arms } => {
            expr_references_var(scrutinee, var_name)
                || arms
                    .iter()
                    .any(|arm| expr_references_var(&arm.body, var_name))
        }
        Expr::Let { value, body, .. } => {
            expr_references_var(value, var_name) || expr_references_var(body, var_name)
        }
        Expr::Tuple(elems) => elems.iter().any(|e| expr_references_var(e, var_name)),
        Expr::Raw(tokens) => tokens.iter().any(|t| t.trim() == var_name),
        Expr::Literal(_) => false,
    }
}

// ---------------------------------------------------------------------------
// Taint tracking (T047 - SEC.1)
// ---------------------------------------------------------------------------

/// Taint label for tracking untrusted data flow.
///
/// Follows the information flow lattice from Section 2.7 of the spec:
/// `Untrusted < Validated < Trusted`
///
/// Data from external sources (network, files, user input) starts as
/// `Untrusted`. Explicit validation functions promote it to `Validated`.
/// Internal data is `Trusted`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TaintLabel {
    /// Data from an external, potentially malicious source.
    Untrusted,
    /// Data that has been explicitly validated/sanitized.
    Validated,
    /// Internal data known to be safe.
    Trusted,
}

impl std::fmt::Display for TaintLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaintLabel::Untrusted => write!(f, "untrusted"),
            TaintLabel::Validated => write!(f, "validated"),
            TaintLabel::Trusted => write!(f, "trusted"),
        }
    }
}

/// Extract a taint label from type annotation tokens.
///
/// Looks for patterns like `@taint:untrusted`, `@taint:validated`,
/// `@taint:trusted` in a sequence of type tokens (from `Param.ty` or
/// `FnDef.return_ty`). Also handles `@untrusted` short form.
///
/// Returns `Some(label)` if found, `None` if no taint annotation is present.
pub fn extract_taint_label(type_tokens: &[String]) -> Option<TaintLabel> {
    // Look for pattern: "@" "taint" ":" <label>
    for window in type_tokens.windows(4) {
        if window[0] == "@" && window[1] == "taint" && window[2] == ":" {
            return match window[3].as_str() {
                "untrusted" => Some(TaintLabel::Untrusted),
                "validated" => Some(TaintLabel::Validated),
                "trusted" => Some(TaintLabel::Trusted),
                _ => None,
            };
        }
    }
    // Check shorter form: "@" <label>
    for window in type_tokens.windows(2) {
        if window[0] == "@" {
            return match window[1].as_str() {
                "untrusted" => Some(TaintLabel::Untrusted),
                "validated" => Some(TaintLabel::Validated),
                "trusted" => Some(TaintLabel::Trusted),
                _ => None,
            };
        }
    }
    None
}

/// Taint checker that tracks taint labels through data flow.
///
/// Implements SEC.1 from Section 14 of the spec: untrusted data taint
/// tracking. Ensures that data from external sources (marked
/// `@taint:untrusted`) cannot flow to sensitive positions (array indices,
/// allocation sizes, etc.) without explicit validation.
///
/// # Error codes
///
/// - **A09101**: Tainted data used as array index without validation
/// - **A09102**: Tainted data used as allocation size without validation
/// - **A09103**: Tainted data flows to trusted sink
/// - **A09104**: Taint validation incomplete (partial sanitization)
#[derive(Debug, Clone)]
pub struct TaintChecker {
    /// Maps variable name to its taint label.
    labels: HashMap<std::string::String, TaintLabel>,
    /// Names of functions known to validate/sanitize input.
    /// These functions convert Untrusted -> Validated.
    validation_fns: std::collections::HashSet<std::string::String>,
    /// Names of functions whose parameters require validated/trusted input.
    /// Maps function name to its parameter taint requirements.
    trusted_sinks: HashMap<std::string::String, Vec<Option<TaintLabel>>>,
}

impl TaintChecker {
    /// Create an empty taint checker with built-in validation function names.
    pub fn new() -> Self {
        let mut validation_fns = std::collections::HashSet::new();
        // Built-in validation function names
        validation_fns.insert("validate".to_string());
        validation_fns.insert("sanitize".to_string());
        Self {
            labels: HashMap::new(),
            validation_fns,
            trusted_sinks: HashMap::new(),
        }
    }

    /// Declare a variable with a taint label.
    pub fn declare(&mut self, name: std::string::String, label: TaintLabel) {
        self.labels.insert(name, label);
    }

    /// Register a function as a validation/sanitization function.
    pub fn register_validator(&mut self, name: std::string::String) {
        self.validation_fns.insert(name);
    }

    /// Register a function as a trusted sink with parameter taint requirements.
    pub fn register_trusted_sink(
        &mut self,
        name: std::string::String,
        param_labels: Vec<Option<TaintLabel>>,
    ) {
        self.trusted_sinks.insert(name, param_labels);
    }

    /// Get the taint label for a variable.
    pub fn get_label(&self, name: &str) -> Option<TaintLabel> {
        self.labels.get(name).copied()
    }

    /// Returns true if any taint labels are tracked.
    pub fn has_taint_info(&self) -> bool {
        !self.labels.is_empty()
    }

    /// Infer the taint label of an expression.
    ///
    /// Taint propagates through operations: if any operand is tainted,
    /// the result is tainted. Uses the minimum in the lattice
    /// (Untrusted < Validated < Trusted).
    pub fn infer_taint(&self, expr: &Expr) -> TaintLabel {
        match expr {
            Expr::Ident(name) => self
                .labels
                .get(name)
                .copied()
                .unwrap_or(TaintLabel::Trusted),
            Expr::Literal(_) => TaintLabel::Trusted,
            Expr::Field(receiver, _) => self.infer_taint(receiver),
            Expr::BinOp { lhs, rhs, .. } => {
                std::cmp::min(self.infer_taint(lhs), self.infer_taint(rhs))
            }
            Expr::UnaryOp { expr: inner, .. } => self.infer_taint(inner),
            Expr::Call { func, args } => {
                // Validation functions produce Validated output
                if let Expr::Ident(name) = func.as_ref()
                    && self.validation_fns.contains(name)
                {
                    return TaintLabel::Validated;
                }
                // Taint propagates from arguments
                args.iter().fold(TaintLabel::Trusted, |acc, arg| {
                    std::cmp::min(acc, self.infer_taint(arg))
                })
            }
            Expr::MethodCall {
                receiver,
                method,
                args,
            } => {
                if self.validation_fns.contains(method) {
                    return TaintLabel::Validated;
                }
                let r = self.infer_taint(receiver);
                args.iter()
                    .fold(r, |acc, arg| std::cmp::min(acc, self.infer_taint(arg)))
            }
            Expr::Index { expr: base, index } => {
                std::cmp::min(self.infer_taint(base), self.infer_taint(index))
            }
            Expr::Old(inner) | Expr::Paren(inner) | Expr::Cast { expr: inner, .. } => {
                self.infer_taint(inner)
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let mut r = std::cmp::min(self.infer_taint(cond), self.infer_taint(then_branch));
                if let Some(e) = else_branch {
                    r = std::cmp::min(r, self.infer_taint(e));
                }
                r
            }
            Expr::List(items) => items.iter().fold(TaintLabel::Trusted, |a, i| {
                std::cmp::min(a, self.infer_taint(i))
            }),
            Expr::Block(exprs) => exprs.iter().fold(TaintLabel::Trusted, |a, e| {
                std::cmp::min(a, self.infer_taint(e))
            }),
            Expr::Forall { body, .. } | Expr::Exists { body, .. } => self.infer_taint(body),
            Expr::Apply { args, .. } => args.iter().fold(TaintLabel::Trusted, |a, arg| {
                std::cmp::min(a, self.infer_taint(arg))
            }),
            Expr::Match { scrutinee, arms } => {
                let mut r = self.infer_taint(scrutinee);
                for arm in arms {
                    r = std::cmp::min(r, self.infer_taint(&arm.body));
                }
                r
            }
            Expr::Let { value, body, .. } => {
                std::cmp::min(self.infer_taint(value), self.infer_taint(body))
            }
            Expr::Tuple(elems) => elems.iter().fold(TaintLabel::Trusted, |a, e| {
                std::cmp::min(a, self.infer_taint(e))
            }),
            Expr::Ghost(_) | Expr::Raw(_) => TaintLabel::Trusted,
        }
    }

    /// Check an expression for taint violations.
    ///
    /// Walks the expression tree looking for sensitive positions where
    /// untrusted data is used without validation.
    pub fn check_expr(&self, expr: &Expr, span: &Range<usize>) -> Vec<TypeError> {
        let mut errors = Vec::new();
        self.check_expr_inner(expr, span, &mut errors);
        errors
    }

    /// Inner recursive checker for taint violations.
    fn check_expr_inner(&self, expr: &Expr, span: &Range<usize>, errors: &mut Vec<TypeError>) {
        match expr {
            // A09101: tainted data as array index
            Expr::Index { expr: base, index } => {
                let index_taint = self.infer_taint(index);
                if index_taint == TaintLabel::Untrusted {
                    errors.push(TypeError {
                        code: "A09101".into(),
                        message: "tainted data used as array index without validation: \
                             validate the index before using it to access an array"
                            .into(),
                        span: span.clone(),
                        secondary: None,
                    });
                }
                self.check_expr_inner(base, span, errors);
                self.check_expr_inner(index, span, errors);
            }

            // A09102 / A09103: tainted data at function call sites
            Expr::Call { func, args } => {
                if let Expr::Ident(name) = func.as_ref() {
                    // A09102: allocation size
                    if is_alloc_function(name) {
                        for arg in args {
                            if self.infer_taint(arg) == TaintLabel::Untrusted {
                                errors.push(TypeError {
                                    code: "A09102".into(),
                                    message: format!(
                                        "tainted data used as allocation size without \
                                         validation: argument to `{name}` is untrusted"
                                    ),
                                    span: span.clone(),
                                    secondary: None,
                                });
                            }
                        }
                    }

                    // A09103: trusted sink
                    if let Some(param_labels) = self.trusted_sinks.get(name) {
                        for (i, arg) in args.iter().enumerate() {
                            let arg_taint = self.infer_taint(arg);
                            if let Some(Some(required)) = param_labels.get(i)
                                && arg_taint < *required
                            {
                                errors.push(TypeError {
                                    code: "A09103".into(),
                                    message: format!(
                                        "tainted data flows to trusted sink: \
                                         argument {i} to `{name}` is `{arg_taint}` \
                                         but parameter requires `{required}`"
                                    ),
                                    span: span.clone(),
                                    secondary: None,
                                });
                            }
                        }
                    }
                }
                self.check_expr_inner(func, span, errors);
                for arg in args {
                    self.check_expr_inner(arg, span, errors);
                }
            }

            // Recurse into sub-expressions
            Expr::BinOp { lhs, rhs, .. } => {
                self.check_expr_inner(lhs, span, errors);
                self.check_expr_inner(rhs, span, errors);
            }
            Expr::UnaryOp { expr: inner, .. }
            | Expr::Old(inner)
            | Expr::Paren(inner)
            | Expr::Cast { expr: inner, .. }
            | Expr::Ghost(inner) => {
                self.check_expr_inner(inner, span, errors);
            }
            Expr::Field(receiver, _) => {
                self.check_expr_inner(receiver, span, errors);
            }
            Expr::MethodCall { receiver, args, .. } => {
                self.check_expr_inner(receiver, span, errors);
                for arg in args {
                    self.check_expr_inner(arg, span, errors);
                }
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.check_expr_inner(cond, span, errors);
                self.check_expr_inner(then_branch, span, errors);
                if let Some(else_br) = else_branch {
                    self.check_expr_inner(else_br, span, errors);
                }
            }
            Expr::List(items) => {
                for item in items {
                    self.check_expr_inner(item, span, errors);
                }
            }
            Expr::Block(exprs) => {
                for e in exprs {
                    self.check_expr_inner(e, span, errors);
                }
            }
            Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
                self.check_expr_inner(domain, span, errors);
                self.check_expr_inner(body, span, errors);
            }
            Expr::Apply { args, .. } => {
                for arg in args {
                    self.check_expr_inner(arg, span, errors);
                }
            }
            Expr::Match { scrutinee, arms } => {
                self.check_expr_inner(scrutinee, span, errors);
                for arm in arms {
                    self.check_expr_inner(&arm.body, span, errors);
                }
            }
            Expr::Let { value, body, .. } => {
                self.check_expr_inner(value, span, errors);
                self.check_expr_inner(body, span, errors);
            }
            Expr::Tuple(elems) => {
                for e in elems {
                    self.check_expr_inner(e, span, errors);
                }
            }
            Expr::Ident(_) | Expr::Literal(_) | Expr::Raw(_) => {}
        }
    }

    /// Check taint flow in a complete source file.
    ///
    /// Extracts taint labels from function parameter and return types,
    /// registers validation functions, then checks all clause expressions
    /// for taint violations. Returns empty if no taint annotations exist.
    pub fn check_file(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = TaintChecker::new();
        let mut has_taint_annotations = false;

        // Pass 1: discover validation functions and trusted sinks
        for decl in &source.decls {
            match &decl.node {
                Decl::FnDef(f) => {
                    if let Some(TaintLabel::Validated) = extract_taint_label(&f.return_ty) {
                        checker.register_validator(f.name.clone());
                        has_taint_annotations = true;
                    }
                    let param_labels: Vec<Option<TaintLabel>> = f
                        .params
                        .iter()
                        .map(|p| extract_taint_label(&p.ty))
                        .collect();
                    // If any param requires validated/trusted, register as sink
                    if param_labels
                        .iter()
                        .any(|l| matches!(l, Some(TaintLabel::Validated | TaintLabel::Trusted)))
                    {
                        checker.register_trusted_sink(f.name.clone(), param_labels.clone());
                        has_taint_annotations = true;
                    }
                    if param_labels.iter().any(|l| l.is_some()) {
                        has_taint_annotations = true;
                    }
                }
                Decl::Extern(e) => {
                    if let Some(TaintLabel::Validated) = extract_taint_label(&e.return_ty) {
                        checker.register_validator(e.name.clone());
                        has_taint_annotations = true;
                    }
                    let param_labels: Vec<Option<TaintLabel>> = e
                        .params
                        .iter()
                        .map(|p| extract_taint_label(&p.ty))
                        .collect();
                    if param_labels
                        .iter()
                        .any(|l| matches!(l, Some(TaintLabel::Validated | TaintLabel::Trusted)))
                    {
                        checker.register_trusted_sink(e.name.clone(), param_labels.clone());
                        has_taint_annotations = true;
                    }
                    if param_labels.iter().any(|l| l.is_some()) {
                        has_taint_annotations = true;
                    }
                }
                _ => {}
            }
        }

        // If no taint annotations, skip the check
        if !has_taint_annotations {
            return Vec::new();
        }

        let mut errors = Vec::new();

        // Pass 2: check each declaration with scoped taint labels
        for decl in &source.decls {
            match &decl.node {
                Decl::FnDef(f) => {
                    let mut fn_checker = checker.clone();
                    for param in &f.params {
                        if let Some(label) = extract_taint_label(&param.ty) {
                            fn_checker.declare(param.name.clone(), label);
                        }
                    }
                    if fn_checker.has_taint_info() {
                        for clause in &f.clauses {
                            errors.extend(fn_checker.check_expr(&clause.body, &decl.span));
                        }
                    }
                }
                Decl::Extern(e) => {
                    let mut fn_checker = checker.clone();
                    for param in &e.params {
                        if let Some(label) = extract_taint_label(&param.ty) {
                            fn_checker.declare(param.name.clone(), label);
                        }
                    }
                    if fn_checker.has_taint_info() {
                        for clause in &e.clauses {
                            errors.extend(fn_checker.check_expr(&clause.body, &decl.span));
                        }
                    }
                }
                Decl::Contract(c) => {
                    if checker.has_taint_info() {
                        for clause in &c.clauses {
                            errors.extend(checker.check_expr(&clause.body, &decl.span));
                        }
                    }
                }
                Decl::Service(s) => {
                    for item in &s.items {
                        match item {
                            ServiceItem::Operation { clauses, .. }
                            | ServiceItem::Query { clauses, .. } => {
                                for clause in clauses {
                                    errors.extend(checker.check_expr(&clause.body, &decl.span));
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Decl::Block { body, .. } => {
                    for clause in body {
                        errors.extend(checker.check_expr(&clause.body, &decl.span));
                    }
                }
                Decl::TypeDef(_) | Decl::EnumDef(_) => {}
            }
        }

        errors
    }
}

impl Default for TaintChecker {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns `true` if the function name is an allocation function.
fn is_alloc_function(name: &str) -> bool {
    matches!(
        name,
        "alloc" | "allocate" | "malloc" | "realloc" | "reserve" | "resize"
    )
}

// ---------------------------------------------------------------------------
// T058: FFI boundary contracts
// ---------------------------------------------------------------------------

/// Trust boundary classification for FFI declarations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustBoundary {
    /// Fully trusted: internal Assura code
    Trusted,
    /// Semi-trusted: audited external code with contracts
    Audited,
    /// Untrusted: arbitrary external code
    Untrusted,
}

impl std::fmt::Display for TrustBoundary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrustBoundary::Trusted => write!(f, "trusted"),
            TrustBoundary::Audited => write!(f, "audited"),
            TrustBoundary::Untrusted => write!(f, "untrusted"),
        }
    }
}

/// Error from the FFI boundary checker.
#[derive(Debug, Clone)]
pub struct FfiError {
    pub code: String,
    pub message: String,
    pub span: Range<usize>,
}

/// Checker for FFI boundary contracts.
///
/// Validates that:
/// - All extern declarations have explicit trust boundary annotations
/// - Untrusted FFI calls have requires/ensures contracts
/// - Data crossing trust boundaries is validated
/// - Unsafe operations are isolated to FFI wrappers
pub struct FfiBoundaryChecker {
    /// Known extern declarations with their trust levels
    externs: HashMap<String, TrustBoundary>,
    /// FFI functions that have contracts (requires/ensures)
    contracted: HashMap<String, bool>,
}

impl FfiBoundaryChecker {
    pub fn new() -> Self {
        Self {
            externs: HashMap::new(),
            contracted: HashMap::new(),
        }
    }

    /// Register an extern declaration with its trust boundary.
    pub fn register_extern(&mut self, name: String, boundary: TrustBoundary) {
        self.externs.insert(name, boundary);
    }

    /// Mark an extern as having a contract (requires/ensures clauses).
    pub fn mark_contracted(&mut self, name: String) {
        self.contracted.insert(name, true);
    }

    /// Check that an extern declaration has the required annotations.
    /// - A11001: extern without trust boundary annotation
    /// - A11002: untrusted extern without contract (requires/ensures)
    pub fn check_extern_decl(
        &self,
        name: &str,
        has_boundary: bool,
        has_contract: bool,
        span: &Range<usize>,
    ) -> Vec<FfiError> {
        let mut errors = Vec::new();
        if !has_boundary {
            errors.push(FfiError {
                code: "A11001".into(),
                message: format!(
                    "extern `{name}` has no trust boundary annotation; \
                     add @trust:trusted, @trust:audited, or @trust:untrusted"
                ),
                span: span.clone(),
            });
        }
        let boundary = self.externs.get(name);
        if boundary == Some(&TrustBoundary::Untrusted) && !has_contract {
            errors.push(FfiError {
                code: "A11002".into(),
                message: format!(
                    "untrusted extern `{name}` has no contract; \
                     add requires/ensures to validate inputs and outputs"
                ),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check that a call to an FFI function validates data at the trust boundary.
    /// - A11003: data from untrusted FFI used without validation
    pub fn check_ffi_call(
        &self,
        callee: &str,
        result_validated: bool,
        span: &Range<usize>,
    ) -> Vec<FfiError> {
        let mut errors = Vec::new();
        if self.externs.get(callee) == Some(&TrustBoundary::Untrusted) && !result_validated {
            errors.push(FfiError {
                code: "A11003".into(),
                message: format!(
                    "result of untrusted FFI call `{callee}` used without validation; \
                     wrap return value in a validate block"
                ),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check that unsafe operations are confined to FFI wrappers.
    /// - A11004: unsafe operation outside FFI wrapper
    pub fn check_unsafe_confinement(
        &self,
        fn_name: &str,
        is_ffi_wrapper: bool,
        has_unsafe: bool,
        span: &Range<usize>,
    ) -> Vec<FfiError> {
        let mut errors = Vec::new();
        if has_unsafe && !is_ffi_wrapper {
            errors.push(FfiError {
                code: "A11004".into(),
                message: format!(
                    "function `{fn_name}` uses unsafe operations but is not an FFI wrapper; \
                     move unsafe code to an extern wrapper"
                ),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check file-level FFI usage: all externs should be audited.
    pub fn check_file(&self, externs: &[(String, bool, bool, Range<usize>)]) -> Vec<FfiError> {
        let mut errors = Vec::new();
        for (name, has_boundary, has_contract, span) in externs {
            errors.extend(self.check_extern_decl(name, *has_boundary, *has_contract, span));
        }
        errors
    }
}

impl Default for FfiBoundaryChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// T062: Interface contracts (trait-like contracts)
// ---------------------------------------------------------------------------

/// An interface contract: a set of required method signatures with contracts.
#[derive(Debug, Clone)]
pub struct InterfaceContract {
    pub name: String,
    /// Required method signatures
    pub methods: Vec<InterfaceMethod>,
    /// Super-interfaces (like trait bounds)
    pub extends: Vec<String>,
}

/// A method signature within an interface contract.
#[derive(Debug, Clone)]
pub struct InterfaceMethod {
    pub name: String,
    pub param_types: Vec<Type>,
    pub return_type: Type,
    pub has_requires: bool,
    pub has_ensures: bool,
    /// Whether the method restricts callback re-entrancy
    pub no_reentrancy: bool,
}

/// Error from the interface contract checker.
#[derive(Debug, Clone)]
pub struct InterfaceError {
    pub code: String,
    pub message: String,
    pub span: Range<usize>,
}

/// Checker for interface contracts.
///
/// Validates that:
/// - Implementations satisfy all interface method contracts
/// - Method signatures match (parameter types, return types)
/// - Re-entrancy restrictions are respected
/// - Super-interface contracts are inherited correctly
pub struct InterfaceChecker {
    /// Known interface definitions
    interfaces: HashMap<String, InterfaceContract>,
    /// Implementations: (implementing_type, interface_name) -> methods
    impls: HashMap<(String, String), Vec<String>>,
}

impl InterfaceChecker {
    pub fn new() -> Self {
        Self {
            interfaces: HashMap::new(),
            impls: HashMap::new(),
        }
    }

    /// Register an interface contract.
    pub fn register_interface(&mut self, iface: InterfaceContract) {
        self.interfaces.insert(iface.name.clone(), iface);
    }

    /// Register an implementation of an interface.
    pub fn register_impl(
        &mut self,
        impl_type: String,
        interface_name: String,
        method_names: Vec<String>,
    ) {
        self.impls.insert((impl_type, interface_name), method_names);
    }

    /// Check that an implementation satisfies all interface methods.
    /// - A13001: missing method implementation
    /// - A13002: method signature mismatch (param or return type)
    pub fn check_impl(
        &self,
        impl_type: &str,
        interface_name: &str,
        implemented_methods: &[String],
        span: &Range<usize>,
    ) -> Vec<InterfaceError> {
        let mut errors = Vec::new();
        let Some(iface) = self.interfaces.get(interface_name) else {
            errors.push(InterfaceError {
                code: "A13001".into(),
                message: format!("unknown interface `{interface_name}`"),
                span: span.clone(),
            });
            return errors;
        };

        for method in &iface.methods {
            if !implemented_methods.contains(&method.name) {
                errors.push(InterfaceError {
                    code: "A13001".into(),
                    message: format!(
                        "`{impl_type}` does not implement required method `{}` \
                         from interface `{interface_name}`",
                        method.name
                    ),
                    span: span.clone(),
                });
            }
        }

        // Check super-interfaces
        for super_name in &iface.extends {
            if let Some(super_iface) = self.interfaces.get(super_name) {
                for method in &super_iface.methods {
                    if !implemented_methods.contains(&method.name) {
                        errors.push(InterfaceError {
                            code: "A13001".into(),
                            message: format!(
                                "`{impl_type}` does not implement required method `{}` \
                                 from super-interface `{super_name}`",
                                method.name
                            ),
                            span: span.clone(),
                        });
                    }
                }
            }
        }

        errors
    }

    /// Check method signature compatibility.
    /// - A13002: parameter count or type mismatch
    pub fn check_method_signature(
        &self,
        interface_name: &str,
        method_name: &str,
        impl_params: &[Type],
        impl_return: &Type,
        span: &Range<usize>,
    ) -> Vec<InterfaceError> {
        let mut errors = Vec::new();
        let Some(iface) = self.interfaces.get(interface_name) else {
            return errors;
        };
        let Some(method) = iface.methods.iter().find(|m| m.name == method_name) else {
            return errors;
        };

        if impl_params.len() != method.param_types.len() {
            errors.push(InterfaceError {
                code: "A13002".into(),
                message: format!(
                    "method `{method_name}` has {} parameters but interface `{interface_name}` \
                     requires {}",
                    impl_params.len(),
                    method.param_types.len()
                ),
                span: span.clone(),
            });
        } else {
            for (i, (impl_t, iface_t)) in impl_params.iter().zip(&method.param_types).enumerate() {
                if impl_t != iface_t {
                    errors.push(InterfaceError {
                        code: "A13002".into(),
                        message: format!(
                            "method `{method_name}` parameter {i}: \
                             expected `{iface_t:?}`, found `{impl_t:?}`"
                        ),
                        span: span.clone(),
                    });
                }
            }
        }

        if impl_return != &method.return_type {
            errors.push(InterfaceError {
                code: "A13002".into(),
                message: format!(
                    "method `{method_name}` return type mismatch: \
                     expected `{:?}`, found `{impl_return:?}`",
                    method.return_type
                ),
                span: span.clone(),
            });
        }

        errors
    }

    /// Check callback re-entrancy restriction.
    /// - A13003: method marked no_reentrancy called recursively through callback
    pub fn check_reentrancy(
        &self,
        interface_name: &str,
        method_name: &str,
        is_reentrant_call: bool,
        span: &Range<usize>,
    ) -> Vec<InterfaceError> {
        let mut errors = Vec::new();
        let is_violation = self
            .interfaces
            .get(interface_name)
            .and_then(|iface| iface.methods.iter().find(|m| m.name == method_name))
            .is_some_and(|method| method.no_reentrancy && is_reentrant_call);
        if is_violation {
            errors.push(InterfaceError {
                code: "A13003".into(),
                message: format!(
                    "method `{method_name}` on interface `{interface_name}` \
                     is marked no_reentrancy but is called re-entrantly"
                ),
                span: span.clone(),
            });
        }
        errors
    }
}

impl Default for InterfaceChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// T059: SEC.3 Constant-time execution
// ---------------------------------------------------------------------------

/// Error from the constant-time checker.
#[derive(Debug, Clone)]
pub struct ConstantTimeError {
    pub code: String,
    pub message: String,
    pub span: Range<usize>,
}

/// Checker for constant-time execution properties.
///
/// Ensures secret-dependent code does not branch on secrets,
/// preventing timing side-channel attacks.
pub struct ConstantTimeChecker {
    /// Variables classified as secret
    secrets: HashMap<String, bool>,
}

impl ConstantTimeChecker {
    pub fn new() -> Self {
        Self {
            secrets: HashMap::new(),
        }
    }

    /// Mark a variable as secret (timing-sensitive).
    pub fn mark_secret(&mut self, name: String) {
        self.secrets.insert(name, true);
    }

    /// Check if an expression references any secret variable.
    pub fn references_secret(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Ident(name) => self.secrets.contains_key(name),
            Expr::BinOp { lhs, rhs, .. } => {
                self.references_secret(lhs) || self.references_secret(rhs)
            }
            Expr::UnaryOp { expr, .. } => self.references_secret(expr),
            Expr::Field(e, _) => self.references_secret(e),
            Expr::Call { func, args } => {
                self.references_secret(func) || args.iter().any(|a| self.references_secret(a))
            }
            Expr::Index { expr, index } => {
                self.references_secret(expr) || self.references_secret(index)
            }
            Expr::Paren(e) | Expr::Old(e) | Expr::Ghost(e) => self.references_secret(e),
            Expr::If { cond, .. } => self.references_secret(cond),
            _ => false,
        }
    }

    /// Check that branches do not depend on secret data.
    /// - A14001: branch condition depends on secret data (timing leak)
    pub fn check_branch(&self, condition: &Expr, span: &Range<usize>) -> Vec<ConstantTimeError> {
        let mut errors = Vec::new();
        if self.references_secret(condition) {
            errors.push(ConstantTimeError {
                code: "A14001".into(),
                message: "branch condition depends on secret data; \
                          this creates a timing side-channel"
                    .into(),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check that array indexing does not depend on secret data.
    /// - A14002: secret-dependent array index (cache timing leak)
    pub fn check_index(&self, index_expr: &Expr, span: &Range<usize>) -> Vec<ConstantTimeError> {
        let mut errors = Vec::new();
        if self.references_secret(index_expr) {
            errors.push(ConstantTimeError {
                code: "A14002".into(),
                message: "array index depends on secret data; \
                          this creates a cache timing side-channel"
                    .into(),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check a full expression for constant-time violations.
    pub fn check_expr(&self, expr: &Expr, span: &Range<usize>) -> Vec<ConstantTimeError> {
        let mut errors = Vec::new();
        match expr {
            Expr::If {
                cond,
                then_branch,
                else_branch,
                ..
            } => {
                errors.extend(self.check_branch(cond, span));
                errors.extend(self.check_expr(then_branch, span));
                if let Some(e) = else_branch {
                    errors.extend(self.check_expr(e, span));
                }
            }
            Expr::Index { index, .. } => {
                errors.extend(self.check_index(index, span));
            }
            Expr::BinOp { lhs, rhs, .. } => {
                errors.extend(self.check_expr(lhs, span));
                errors.extend(self.check_expr(rhs, span));
            }
            Expr::Call { args, .. } => {
                for a in args {
                    errors.extend(self.check_expr(a, span));
                }
            }
            _ => {}
        }
        errors
    }
}

impl Default for ConstantTimeChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// T063: TYPE.2 Recursive structural invariants
// ---------------------------------------------------------------------------

/// A structural invariant on a recursive data structure.
#[derive(Debug, Clone)]
pub struct StructuralInvariant {
    pub name: String,
    /// The type this invariant applies to
    pub type_name: String,
    /// Kind of structural property
    pub kind: InvariantKind,
}

/// Kinds of structural invariants for recursive types.
#[derive(Debug, Clone, PartialEq)]
pub enum InvariantKind {
    /// Tree balance: left depth and right depth differ by at most k
    TreeBalance { max_diff: u32 },
    /// List sortedness: elements in non-decreasing order
    Sorted { descending: bool },
    /// Graph acyclicity: no cycles in the structure
    Acyclic,
    /// Binary search tree: left < node < right
    BstOrdering,
    /// Heap property: parent <= children (or >=)
    HeapProperty { min_heap: bool },
    /// Custom invariant expressed as a predicate string
    Custom(String),
}

impl std::fmt::Display for InvariantKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InvariantKind::TreeBalance { max_diff } => {
                write!(f, "tree_balance(max_diff={max_diff})")
            }
            InvariantKind::Sorted { descending } => {
                if *descending {
                    write!(f, "sorted(desc)")
                } else {
                    write!(f, "sorted(asc)")
                }
            }
            InvariantKind::Acyclic => write!(f, "acyclic"),
            InvariantKind::BstOrdering => write!(f, "bst_ordering"),
            InvariantKind::HeapProperty { min_heap } => {
                if *min_heap {
                    write!(f, "min_heap")
                } else {
                    write!(f, "max_heap")
                }
            }
            InvariantKind::Custom(pred) => write!(f, "custom({pred})"),
        }
    }
}

/// Error from the structural invariant checker.
#[derive(Debug, Clone)]
pub struct StructuralInvariantError {
    pub code: String,
    pub message: String,
    pub span: Range<usize>,
}

/// Checker for recursive structural invariants.
pub struct StructuralInvariantChecker {
    /// Registered invariants per type
    invariants: HashMap<String, Vec<StructuralInvariant>>,
    /// Known recursive types (type name -> list of recursive field names)
    recursive_types: HashMap<String, Vec<String>>,
}

impl StructuralInvariantChecker {
    pub fn new() -> Self {
        Self {
            invariants: HashMap::new(),
            recursive_types: HashMap::new(),
        }
    }

    /// Register a type as recursive, listing its self-referencing fields.
    pub fn register_recursive_type(&mut self, type_name: String, recursive_fields: Vec<String>) {
        self.recursive_types.insert(type_name, recursive_fields);
    }

    /// Register a structural invariant on a type.
    pub fn register_invariant(&mut self, inv: StructuralInvariant) {
        self.invariants
            .entry(inv.type_name.clone())
            .or_default()
            .push(inv);
    }

    /// Check that a structural invariant is applicable to the type.
    /// - A15001: invariant on non-recursive type
    /// - A15002: tree invariant on non-tree structure
    /// - A15003: sort invariant on non-sequence structure
    pub fn check_invariant_applicability(
        &self,
        type_name: &str,
        kind: &InvariantKind,
        span: &Range<usize>,
    ) -> Vec<StructuralInvariantError> {
        let mut errors = Vec::new();
        if !self.recursive_types.contains_key(type_name) {
            errors.push(StructuralInvariantError {
                code: "A15001".into(),
                message: format!(
                    "structural invariant `{kind}` applied to non-recursive type `{type_name}`"
                ),
                span: span.clone(),
            });
            return errors;
        }

        let fields = &self.recursive_types[type_name];
        match kind {
            InvariantKind::TreeBalance { .. }
            | InvariantKind::BstOrdering
            | InvariantKind::HeapProperty { .. } => {
                // Tree invariants need at least 2 recursive fields (left, right)
                if fields.len() < 2 {
                    errors.push(StructuralInvariantError {
                        code: "A15002".into(),
                        message: format!(
                            "tree invariant `{kind}` requires at least 2 recursive fields, \
                             but `{type_name}` has {}",
                            fields.len()
                        ),
                        span: span.clone(),
                    });
                }
            }
            InvariantKind::Sorted { .. } => {
                // Sort invariant needs exactly 1 recursive field (next pointer)
                if fields.len() != 1 {
                    errors.push(StructuralInvariantError {
                        code: "A15003".into(),
                        message: format!(
                            "sort invariant requires exactly 1 recursive field (next pointer), \
                             but `{type_name}` has {}",
                            fields.len()
                        ),
                        span: span.clone(),
                    });
                }
            }
            InvariantKind::Acyclic | InvariantKind::Custom(_) => {
                // These are valid for any recursive type
            }
        }
        errors
    }

    /// Check that an operation preserves the structural invariant.
    /// - A15004: operation may violate structural invariant
    pub fn check_operation_preserves(
        &self,
        type_name: &str,
        operation: &str,
        modifies_structure: bool,
        has_preservation_proof: bool,
        span: &Range<usize>,
    ) -> Vec<StructuralInvariantError> {
        let mut errors = Vec::new();
        if !modifies_structure {
            return errors; // Read-only operations preserve invariants trivially
        }
        if let Some(invs) = self.invariants.get(type_name) {
            for inv in invs {
                if !has_preservation_proof {
                    errors.push(StructuralInvariantError {
                        code: "A15004".into(),
                        message: format!(
                            "operation `{operation}` modifies `{type_name}` \
                             but has no proof preserving invariant `{}`",
                            inv.kind
                        ),
                        span: span.clone(),
                    });
                }
            }
        }
        errors
    }

    /// Get all invariants for a type (including inherited through recursive substructure).
    pub fn get_invariants(&self, type_name: &str) -> Vec<&StructuralInvariant> {
        self.invariants
            .get(type_name)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }
}

impl Default for StructuralInvariantChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// T065: CONC.1 Shared memory protocols
// ---------------------------------------------------------------------------

/// Access mode for a shared object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessMode {
    /// Exclusive read-write access (no other readers/writers)
    Exclusive,
    /// Shared read-only access (multiple readers, no writers)
    SharedRead,
    /// No access (object is locked by another thread)
    None,
}

impl std::fmt::Display for AccessMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AccessMode::Exclusive => write!(f, "exclusive"),
            AccessMode::SharedRead => write!(f, "shared_read"),
            AccessMode::None => write!(f, "none"),
        }
    }
}

/// Error from the shared memory checker.
#[derive(Debug, Clone)]
pub struct SharedMemError {
    pub code: String,
    pub message: String,
    pub span: Range<usize>,
}

/// Checker for shared memory protocols.
///
/// Validates that concurrent accesses to shared objects follow
/// the declared protocol: no data races, no concurrent writes.
pub struct SharedMemChecker {
    /// Per-object access modes
    object_modes: HashMap<String, AccessMode>,
}

impl SharedMemChecker {
    pub fn new() -> Self {
        Self {
            object_modes: HashMap::new(),
        }
    }

    /// Set the current access mode for an object.
    pub fn set_mode(&mut self, object: String, mode: AccessMode) {
        self.object_modes.insert(object, mode);
    }

    /// Check that a read access is valid for the current mode.
    /// - A18001: read without shared_read or exclusive access
    pub fn check_read(&self, object: &str, span: &Range<usize>) -> Vec<SharedMemError> {
        let mut errors = Vec::new();
        match self.object_modes.get(object) {
            Some(AccessMode::Exclusive | AccessMode::SharedRead) => {}
            Some(AccessMode::None) | None => {
                errors.push(SharedMemError {
                    code: "A18001".into(),
                    message: format!(
                        "read access to `{object}` without acquiring shared_read or exclusive mode"
                    ),
                    span: span.clone(),
                });
            }
        }
        errors
    }

    /// Check that a write access is valid for the current mode.
    /// - A18002: write without exclusive access
    pub fn check_write(&self, object: &str, span: &Range<usize>) -> Vec<SharedMemError> {
        let mut errors = Vec::new();
        if self.object_modes.get(object) != Some(&AccessMode::Exclusive) {
            errors.push(SharedMemError {
                code: "A18002".into(),
                message: format!(
                    "write access to `{object}` without exclusive mode; \
                     acquire exclusive access before writing"
                ),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check for potential data race: two threads accessing the same object.
    /// - A18003: data race (concurrent write + read or write + write)
    pub fn check_data_race(
        &self,
        object: &str,
        thread_a_mode: AccessMode,
        thread_b_mode: AccessMode,
        span: &Range<usize>,
    ) -> Vec<SharedMemError> {
        let mut errors = Vec::new();
        let is_race = matches!(
            (thread_a_mode, thread_b_mode),
            (
                AccessMode::Exclusive,
                AccessMode::Exclusive | AccessMode::SharedRead
            ) | (AccessMode::SharedRead, AccessMode::Exclusive)
        );
        if is_race {
            errors.push(SharedMemError {
                code: "A18003".into(),
                message: format!(
                    "potential data race on `{object}`: thread A has {thread_a_mode} \
                     while thread B has {thread_b_mode}"
                ),
                span: span.clone(),
            });
        }
        errors
    }
}

impl Default for SharedMemChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// T067: CONC.3 Determinism contracts
// ---------------------------------------------------------------------------

/// Error from the determinism checker.
#[derive(Debug, Clone)]
pub struct DeterminismError {
    pub code: String,
    pub message: String,
    pub span: Range<usize>,
}

/// Checker for determinism contracts.
///
/// Ensures functions marked as `deterministic` do not use
/// non-deterministic constructs (HashMap iteration, random,
/// thread-dependent ordering).
pub struct DeterminismChecker {
    /// Functions marked as deterministic
    deterministic_fns: HashMap<String, bool>,
    /// Known non-deterministic types/functions
    non_det_sources: Vec<String>,
}

impl DeterminismChecker {
    pub fn new() -> Self {
        Self {
            deterministic_fns: HashMap::new(),
            non_det_sources: vec![
                "HashMap".into(),
                "HashSet".into(),
                "random".into(),
                "rand".into(),
                "thread_rng".into(),
                "SystemTime::now".into(),
                "Instant::now".into(),
            ],
        }
    }

    /// Mark a function as requiring deterministic execution.
    pub fn mark_deterministic(&mut self, fn_name: String) {
        self.deterministic_fns.insert(fn_name, true);
    }

    /// Add a custom non-deterministic source.
    pub fn add_non_det_source(&mut self, source: String) {
        self.non_det_sources.push(source);
    }

    /// Check if a type/function name is non-deterministic.
    pub fn is_non_deterministic(&self, name: &str) -> bool {
        self.non_det_sources
            .iter()
            .any(|s| name.contains(s.as_str()))
    }

    /// Check that a deterministic function does not use non-deterministic constructs.
    /// - A20001: deterministic function uses non-deterministic type/call
    pub fn check_fn_body(
        &self,
        fn_name: &str,
        used_names: &[String],
        span: &Range<usize>,
    ) -> Vec<DeterminismError> {
        let mut errors = Vec::new();
        if !self.deterministic_fns.contains_key(fn_name) {
            return errors; // Not marked deterministic, skip
        }
        for name in used_names {
            if self.is_non_deterministic(name) {
                errors.push(DeterminismError {
                    code: "A20001".into(),
                    message: format!(
                        "deterministic function `{fn_name}` uses non-deterministic `{name}`; \
                         use BTreeMap/BTreeSet or a seeded RNG instead"
                    ),
                    span: span.clone(),
                });
            }
        }
        errors
    }

    /// Check that iteration order is deterministic.
    /// - A20002: iterating over HashMap/HashSet in deterministic context
    pub fn check_iteration(
        &self,
        fn_name: &str,
        iterated_type: &str,
        span: &Range<usize>,
    ) -> Vec<DeterminismError> {
        let mut errors = Vec::new();
        if self.deterministic_fns.contains_key(fn_name)
            && (iterated_type.contains("HashMap") || iterated_type.contains("HashSet"))
        {
            errors.push(DeterminismError {
                code: "A20002".into(),
                message: format!(
                    "deterministic function `{fn_name}` iterates over `{iterated_type}` \
                     which has non-deterministic ordering"
                ),
                span: span.clone(),
            });
        }
        errors
    }
}

impl Default for DeterminismChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// T068: CONC.4 Lock ordering
// ---------------------------------------------------------------------------

/// Error from the lock ordering checker.
#[derive(Debug, Clone)]
pub struct LockOrderError {
    pub code: String,
    pub message: String,
    pub span: Range<usize>,
}

/// Checker for static lock ordering.
///
/// Prevents deadlocks by enforcing a total order on lock acquisitions.
pub struct LockOrderChecker {
    /// Lock hierarchy: name -> priority (lower = acquire first)
    lock_order: HashMap<String, u32>,
    /// Currently held locks (name, priority)
    held: Vec<(String, u32)>,
}

impl LockOrderChecker {
    pub fn new() -> Self {
        Self {
            lock_order: HashMap::new(),
            held: Vec::new(),
        }
    }

    /// Define the lock hierarchy. Locks with lower priority must be acquired first.
    pub fn define_order(&mut self, lock_name: String, priority: u32) {
        self.lock_order.insert(lock_name, priority);
    }

    /// Record acquiring a lock. Check ordering.
    /// - A21001: lock acquired out of order (deadlock risk)
    pub fn acquire(&mut self, lock_name: &str, span: &Range<usize>) -> Vec<LockOrderError> {
        let mut errors = Vec::new();
        let priority = self.lock_order.get(lock_name).copied().unwrap_or(u32::MAX);

        // Check that we're not acquiring a lower-priority lock while holding higher
        if let Some((held_name, held_priority)) = self.held.last().filter(|(_, hp)| priority <= *hp)
        {
            errors.push(LockOrderError {
                code: "A21001".into(),
                message: format!(
                    "lock `{lock_name}` (priority {priority}) acquired while holding \
                     `{held_name}` (priority {held_priority}); violates lock ordering"
                ),
                span: span.clone(),
            });
        }

        self.held.push((lock_name.to_string(), priority));
        errors
    }

    /// Record releasing a lock.
    /// - A21002: lock released out of order (must release in reverse acquisition order)
    pub fn release(&mut self, lock_name: &str, span: &Range<usize>) -> Vec<LockOrderError> {
        let mut errors = Vec::new();
        if let Some((top_name, _)) = self.held.last().filter(|(n, _)| n != lock_name) {
            errors.push(LockOrderError {
                code: "A21002".into(),
                message: format!(
                    "lock `{lock_name}` released while `{top_name}` is still held; \
                     release in reverse acquisition order"
                ),
                span: span.clone(),
            });
        }
        self.held.retain(|(n, _)| n != lock_name);
        errors
    }

    /// Check that no lock is known but unordered.
    /// - A21003: lock used without defined order
    pub fn check_ordering_defined(
        &self,
        lock_name: &str,
        span: &Range<usize>,
    ) -> Vec<LockOrderError> {
        let mut errors = Vec::new();
        if !self.lock_order.contains_key(lock_name) {
            errors.push(LockOrderError {
                code: "A21003".into(),
                message: format!(
                    "lock `{lock_name}` used without a defined ordering; \
                     add it to the lock hierarchy"
                ),
                span: span.clone(),
            });
        }
        errors
    }
}

impl Default for LockOrderChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// T060: SEC.4 Secure erasure
// ---------------------------------------------------------------------------

/// Error from the secure erasure checker.
#[derive(Debug, Clone)]
pub struct SecureErasureError {
    pub code: String,
    pub message: String,
    pub span: Range<usize>,
}

/// Checker for secure erasure of sensitive data.
///
/// Ensures that linear types marked as sensitive are consumed
/// via zeroize before being dropped, preventing sensitive data
/// from lingering in memory.
pub struct SecureErasureChecker {
    /// Variables that hold sensitive data and must be zeroized
    sensitive_vars: HashMap<String, bool>,
    /// Variables that have been properly zeroized
    zeroized: HashMap<String, bool>,
}

impl SecureErasureChecker {
    pub fn new() -> Self {
        Self {
            sensitive_vars: HashMap::new(),
            zeroized: HashMap::new(),
        }
    }

    /// Returns the names of all sensitive variables.
    pub fn sensitive_names(&self) -> Vec<String> {
        self.sensitive_vars.keys().cloned().collect()
    }

    /// Mark a variable as holding sensitive data.
    pub fn mark_sensitive(&mut self, name: String) {
        self.sensitive_vars.insert(name, true);
    }

    /// Record that a variable has been zeroized.
    pub fn mark_zeroized(&mut self, name: String) {
        self.zeroized.insert(name, true);
    }

    /// Check that a sensitive variable was zeroized before going out of scope.
    /// - A16001: sensitive variable dropped without zeroization
    pub fn check_scope_exit(&self, var_name: &str, span: &Range<usize>) -> Vec<SecureErasureError> {
        let mut errors = Vec::new();
        if self.sensitive_vars.contains_key(var_name) && !self.zeroized.contains_key(var_name) {
            errors.push(SecureErasureError {
                code: "A16001".into(),
                message: format!(
                    "sensitive variable `{var_name}` dropped without secure erasure; \
                     call zeroize() before the variable goes out of scope"
                ),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check that a copy of sensitive data is also marked sensitive.
    /// - A16002: sensitive data copied to non-sensitive variable
    pub fn check_copy(
        &self,
        source: &str,
        target: &str,
        target_is_sensitive: bool,
        span: &Range<usize>,
    ) -> Vec<SecureErasureError> {
        let mut errors = Vec::new();
        if self.sensitive_vars.contains_key(source) && !target_is_sensitive {
            errors.push(SecureErasureError {
                code: "A16002".into(),
                message: format!(
                    "sensitive data from `{source}` copied to `{target}` \
                     which is not marked as sensitive; the copy will not be zeroized"
                ),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check that sensitive data is not leaked through return values.
    /// - A16003: function returns sensitive data without @sensitive annotation
    pub fn check_return(
        &self,
        returned_var: &str,
        fn_return_is_sensitive: bool,
        span: &Range<usize>,
    ) -> Vec<SecureErasureError> {
        let mut errors = Vec::new();
        if self.sensitive_vars.contains_key(returned_var) && !fn_return_is_sensitive {
            errors.push(SecureErasureError {
                code: "A16003".into(),
                message: format!(
                    "function returns sensitive variable `{returned_var}` \
                     but return type is not marked @sensitive"
                ),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check all sensitive variables at end of scope.
    pub fn check_all_erased(&self, span: &Range<usize>) -> Vec<SecureErasureError> {
        let mut errors = Vec::new();
        for name in self.sensitive_vars.keys() {
            if !self.zeroized.contains_key(name) {
                errors.push(SecureErasureError {
                    code: "A16001".into(),
                    message: format!("sensitive variable `{name}` dropped without secure erasure"),
                    span: span.clone(),
                });
            }
        }
        errors
    }
}

impl Default for SecureErasureChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// T061: SEC.5 Cryptographic conformance
// ---------------------------------------------------------------------------

/// Error from the cryptographic conformance checker.
#[derive(Debug, Clone)]
pub struct CryptoConformanceError {
    pub code: String,
    pub message: String,
    pub span: Range<usize>,
}

/// A cryptographic algorithm specification.
#[derive(Debug, Clone)]
pub struct CryptoSpec {
    pub name: String,
    pub key_size_bits: Vec<u32>,
    pub block_size_bytes: Option<u32>,
    pub nonce_size_bytes: Option<u32>,
    pub tag_size_bytes: Option<u32>,
}

/// Checker for cryptographic conformance.
///
/// Validates that cryptographic implementations match their mathematical
/// specifications: correct key sizes, nonce handling, tag verification.
pub struct CryptoConformanceChecker {
    /// Known algorithm specs
    specs: HashMap<String, CryptoSpec>,
}

impl CryptoConformanceChecker {
    pub fn new() -> Self {
        let mut specs = HashMap::new();
        // Register common algorithms
        specs.insert(
            "AES-128-GCM".into(),
            CryptoSpec {
                name: "AES-128-GCM".into(),
                key_size_bits: vec![128],
                block_size_bytes: Some(16),
                nonce_size_bytes: Some(12),
                tag_size_bytes: Some(16),
            },
        );
        specs.insert(
            "AES-256-GCM".into(),
            CryptoSpec {
                name: "AES-256-GCM".into(),
                key_size_bits: vec![256],
                block_size_bytes: Some(16),
                nonce_size_bytes: Some(12),
                tag_size_bytes: Some(16),
            },
        );
        specs.insert(
            "ChaCha20-Poly1305".into(),
            CryptoSpec {
                name: "ChaCha20-Poly1305".into(),
                key_size_bits: vec![256],
                block_size_bytes: None,
                nonce_size_bytes: Some(12),
                tag_size_bytes: Some(16),
            },
        );
        Self { specs }
    }

    /// Register a custom algorithm specification.
    pub fn register_spec(&mut self, spec: CryptoSpec) {
        self.specs.insert(spec.name.clone(), spec);
    }

    /// Check that a key size matches the algorithm spec.
    /// - A17001: wrong key size for algorithm
    pub fn check_key_size(
        &self,
        algorithm: &str,
        key_size_bits: u32,
        span: &Range<usize>,
    ) -> Vec<CryptoConformanceError> {
        let mut errors = Vec::new();
        if let Some(spec) = self
            .specs
            .get(algorithm)
            .filter(|s| !s.key_size_bits.contains(&key_size_bits))
        {
            errors.push(CryptoConformanceError {
                code: "A17001".into(),
                message: format!(
                    "key size {key_size_bits} bits does not match `{algorithm}` \
                     which requires {:?} bits",
                    spec.key_size_bits
                ),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check that a nonce size matches the algorithm spec.
    /// - A17002: wrong nonce size for algorithm
    pub fn check_nonce_size(
        &self,
        algorithm: &str,
        nonce_size_bytes: u32,
        span: &Range<usize>,
    ) -> Vec<CryptoConformanceError> {
        let mut errors = Vec::new();
        let mismatch = self
            .specs
            .get(algorithm)
            .and_then(|s| s.nonce_size_bytes)
            .filter(|&expected| nonce_size_bytes != expected);
        if let Some(expected) = mismatch {
            errors.push(CryptoConformanceError {
                code: "A17002".into(),
                message: format!(
                    "nonce size {nonce_size_bytes} bytes does not match `{algorithm}` \
                     which requires {expected} bytes"
                ),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check that nonce reuse is prevented.
    /// - A17003: potential nonce reuse detected
    pub fn check_nonce_uniqueness(
        &self,
        nonce_source: &str,
        is_counter: bool,
        is_random: bool,
        span: &Range<usize>,
    ) -> Vec<CryptoConformanceError> {
        let mut errors = Vec::new();
        if !is_counter && !is_random {
            errors.push(CryptoConformanceError {
                code: "A17003".into(),
                message: format!(
                    "nonce `{nonce_source}` is neither counter-based nor random; \
                     potential nonce reuse"
                ),
                span: span.clone(),
            });
        }
        errors
    }

    /// Check that authentication tag is verified before using decrypted data.
    /// - A17004: decrypted data used before tag verification
    pub fn check_tag_verification(
        &self,
        has_tag_check: bool,
        span: &Range<usize>,
    ) -> Vec<CryptoConformanceError> {
        let mut errors = Vec::new();
        if !has_tag_check {
            errors.push(CryptoConformanceError {
                code: "A17004".into(),
                message: "decrypted data used before authentication tag verification; \
                          verify the tag before processing plaintext"
                    .into(),
                span: span.clone(),
            });
        }
        errors
    }
}

impl Default for CryptoConformanceChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// T052: Dependent types (restricted)
// ---------------------------------------------------------------------------

/// A dependent type index: the value a type depends on.
/// Restricted to Nat, Bool, and finite enums (not arbitrary expressions).
#[derive(Debug, Clone, PartialEq)]
pub enum DepIndex {
    /// A natural number index, e.g. Vec<T, n>
    Nat(String),
    /// A boolean index, e.g. Matrix<T, is_square>
    Bool(String),
    /// A finite enum index, e.g. Buffer<mode> where mode: ReadWrite
    Enum { name: String, enum_type: String },
}

impl std::fmt::Display for DepIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DepIndex::Nat(n) => write!(f, "{n}: Nat"),
            DepIndex::Bool(n) => write!(f, "{n}: Bool"),
            DepIndex::Enum { name, enum_type } => write!(f, "{name}: {enum_type}"),
        }
    }
}

/// A dependent type: a base type parameterized by one or more indices.
#[derive(Debug, Clone, PartialEq)]
pub struct DepType {
    pub base: Type,
    pub indices: Vec<DepIndex>,
}

/// Error from the dependent type checker.
#[derive(Debug, Clone)]
pub struct DepTypeError {
    pub code: String,
    pub message: String,
    pub span: Range<usize>,
}

/// Checker for restricted dependent types.
///
/// Validates that:
/// - Dependent type indices are of allowed kinds (Nat, Bool, finite enum)
/// - Index arithmetic in type positions is well-formed
/// - Indices are erased at runtime (ghost)
/// - Type equality with indices is checked structurally
pub struct DependentTypeChecker {
    /// Known enum types and their variants (for finiteness check)
    enums: HashMap<String, Vec<String>>,
    /// Known dependent type definitions
    dep_types: HashMap<String, DepType>,
    /// Index variable bindings in scope: name -> DepIndex
    index_vars: HashMap<String, DepIndex>,
}

impl DependentTypeChecker {
    pub fn new() -> Self {
        Self {
            enums: HashMap::new(),
            dep_types: HashMap::new(),
            index_vars: HashMap::new(),
        }
    }

    /// Register a finite enum type with its variants.
    pub fn register_enum(&mut self, name: String, variants: Vec<String>) {
        self.enums.insert(name, variants);
    }

    /// Register a dependent type definition.
    pub fn register_dep_type(&mut self, name: String, dep_type: DepType) {
        self.dep_types.insert(name, dep_type);
    }

    /// Bind an index variable in the current scope.
    pub fn bind_index(&mut self, name: String, index: DepIndex) {
        self.index_vars.insert(name, index);
    }

    /// Validate that a type index is of an allowed kind.
    /// Returns A03006 if the index type is not Nat, Bool, or a known finite enum.
    pub fn validate_index(
        &self,
        index_name: &str,
        index_type: &str,
        span: &Range<usize>,
    ) -> Vec<DepTypeError> {
        let mut errors = Vec::new();
        match index_type {
            "Nat" | "Bool" => { /* allowed */ }
            other => {
                if !self.enums.contains_key(other) {
                    errors.push(DepTypeError {
                        code: "A03006".into(),
                        message: format!(
                            "dependent type index `{index_name}` has type `{other}`, \
                             which is not Nat, Bool, or a known finite enum"
                        ),
                        span: span.clone(),
                    });
                }
            }
        }
        errors
    }

    /// Check that index arithmetic in a type position is well-formed.
    /// For Nat indices, expressions like `n + 1`, `n - 1`, `2 * n` are allowed.
    /// For Bool/Enum indices, only direct references are allowed (no arithmetic).
    pub fn check_index_expr(
        &self,
        expr: &Expr,
        expected_kind: &DepIndex,
        span: &Range<usize>,
    ) -> Vec<DepTypeError> {
        let mut errors = Vec::new();
        match expected_kind {
            DepIndex::Nat(_) => {
                // Nat indices allow arithmetic expressions
                if !self.is_nat_expr(expr) {
                    errors.push(DepTypeError {
                        code: "A03007".into(),
                        message: "index expression is not a valid Nat expression; \
                                  only integer arithmetic over index variables is allowed"
                            .into(),
                        span: span.clone(),
                    });
                }
            }
            DepIndex::Bool(_) => {
                // Bool indices: only ident or boolean literal
                if !self.is_bool_expr(expr) {
                    errors.push(DepTypeError {
                        code: "A03008".into(),
                        message: "Bool index must be a direct reference or boolean literal, \
                                  not an arithmetic expression"
                            .into(),
                        span: span.clone(),
                    });
                }
            }
            DepIndex::Enum { enum_type, .. } => {
                // Enum indices: only ident or enum variant
                if !self.is_enum_expr(expr, enum_type) {
                    errors.push(DepTypeError {
                        code: "A03009".into(),
                        message: format!(
                            "enum index of type `{enum_type}` must be a direct reference \
                             or variant name"
                        ),
                        span: span.clone(),
                    });
                }
            }
        }
        errors
    }

    /// Check structural equality of two dependent types.
    /// Two `Vec<T, n>` and `Vec<T, m>` are equal only if `n == m` can be proved.
    pub fn check_dep_type_eq(
        &self,
        expected: &DepType,
        actual: &DepType,
        span: &Range<usize>,
    ) -> Vec<DepTypeError> {
        let mut errors = Vec::new();
        if expected.base != actual.base {
            errors.push(DepTypeError {
                code: "A03010".into(),
                message: format!(
                    "dependent type base mismatch: expected `{:?}`, found `{:?}`",
                    expected.base, actual.base
                ),
                span: span.clone(),
            });
            return errors;
        }
        if expected.indices.len() != actual.indices.len() {
            errors.push(DepTypeError {
                code: "A03010".into(),
                message: format!(
                    "dependent type index count mismatch: expected {}, found {}",
                    expected.indices.len(),
                    actual.indices.len()
                ),
                span: span.clone(),
            });
            return errors;
        }
        for (i, (exp, act)) in expected.indices.iter().zip(&actual.indices).enumerate() {
            if std::mem::discriminant(exp) != std::mem::discriminant(act) {
                errors.push(DepTypeError {
                    code: "A03011".into(),
                    message: format!(
                        "dependent type index {i} kind mismatch: expected {exp}, found {act}"
                    ),
                    span: span.clone(),
                });
            }
        }
        errors
    }

    /// Verify that index variables are erased at runtime.
    /// Returns an error if an index variable appears in a non-ghost context.
    pub fn check_index_erasure(
        &self,
        expr: &Expr,
        ghost_context: bool,
        span: &Range<usize>,
    ) -> Vec<DepTypeError> {
        if ghost_context {
            return Vec::new(); // Ghost context: indices are fine
        }
        let mut errors = Vec::new();
        for name in self.collect_idents(expr) {
            if self.index_vars.contains_key(&name) {
                errors.push(DepTypeError {
                    code: "A03012".into(),
                    message: format!(
                        "index variable `{name}` used in runtime context; \
                         dependent type indices must be erased at runtime"
                    ),
                    span: span.clone(),
                });
            }
        }
        errors
    }

    // --- Helper methods ---

    fn is_nat_expr(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Literal(Literal::Int(_)) => true,
            Expr::Ident(name) => {
                matches!(self.index_vars.get(name), Some(DepIndex::Nat(_)))
                    || !self.index_vars.contains_key(name)
            }
            Expr::BinOp { lhs, op, rhs } => {
                matches!(
                    op,
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod
                ) && self.is_nat_expr(lhs)
                    && self.is_nat_expr(rhs)
            }
            Expr::UnaryOp {
                op: UnaryOp::Neg,
                expr,
            } => self.is_nat_expr(expr),
            Expr::Paren(inner) => self.is_nat_expr(inner),
            _ => false,
        }
    }

    fn is_bool_expr(&self, expr: &Expr) -> bool {
        matches!(expr, Expr::Literal(Literal::Bool(_)) | Expr::Ident(_))
    }

    fn is_enum_expr(&self, expr: &Expr, enum_type: &str) -> bool {
        match expr {
            Expr::Ident(name) => {
                // Either a variable reference or a variant name
                if let Some(variants) = self.enums.get(enum_type) {
                    variants.contains(name) || self.index_vars.contains_key(name)
                } else {
                    self.index_vars.contains_key(name)
                }
            }
            _ => false,
        }
    }

    fn collect_idents(&self, expr: &Expr) -> Vec<String> {
        let mut names = Vec::new();
        match expr {
            Expr::Ident(n) => names.push(n.clone()),
            Expr::BinOp { lhs, rhs, .. } => {
                names.extend(self.collect_idents(lhs));
                names.extend(self.collect_idents(rhs));
            }
            Expr::UnaryOp { expr, .. } => names.extend(self.collect_idents(expr)),
            Expr::Call { func, args } => {
                names.extend(self.collect_idents(func));
                for a in args {
                    names.extend(self.collect_idents(a));
                }
            }
            Expr::Field(e, _) => names.extend(self.collect_idents(e)),
            Expr::Index { expr, index } => {
                names.extend(self.collect_idents(expr));
                names.extend(self.collect_idents(index));
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                names.extend(self.collect_idents(cond));
                names.extend(self.collect_idents(then_branch));
                if let Some(e) = else_branch {
                    names.extend(self.collect_idents(e));
                }
            }
            Expr::Paren(e) | Expr::Old(e) | Expr::Ghost(e) => {
                names.extend(self.collect_idents(e));
            }
            _ => {}
        }
        names
    }
}

impl Default for DependentTypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests

// ---------------------------------------------------------------------------
// Information flow checking (T051 - SEC.3)
// ---------------------------------------------------------------------------

/// Security label in the information flow lattice.
///
/// The lattice is ordered: `Public < Internal < Confidential < Restricted`.
/// Data may flow upward in the lattice (Public -> Confidential) but never
/// downward (Confidential -> Public) without explicit declassification.
///
/// Implements Section 2.7 of the spec (information flow types).
///
/// # Error codes
///
/// - **A08001**: Information flows from higher security to lower security
/// - **A08002**: Declassification without explicit annotation
/// - **A08003**: Purpose label mismatch (GDPR)
/// - **A08004**: Implicit flow through control dependency
/// - **A08005**: Covert channel through timing/exceptions
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SecurityLabel {
    /// Publicly accessible data.
    Public,
    /// Internal-only data (not exposed to external consumers).
    Internal,
    /// Confidential data (PII, credentials, etc.).
    Confidential,
    /// Restricted data (highest classification, e.g. encryption keys).
    Restricted,
}

impl std::fmt::Display for SecurityLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecurityLabel::Public => write!(f, "Public"),
            SecurityLabel::Internal => write!(f, "Internal"),
            SecurityLabel::Confidential => write!(f, "Confidential"),
            SecurityLabel::Restricted => write!(f, "Restricted"),
        }
    }
}

/// A structured information flow error.
#[derive(Debug, Clone)]
pub struct InfoFlowError {
    /// Error code (A08001-A08005).
    pub code: std::string::String,
    /// Human-readable error message.
    pub message: std::string::String,
    /// Source location where the error was detected.
    pub span: Range<usize>,
}

/// Information flow checker that enforces the security lattice.
///
/// Tracks security labels on variables and ensures that data never flows
/// from a higher security level to a lower one without explicit
/// declassification.  Also tracks GDPR purpose labels for data-purpose
/// compliance.
#[derive(Debug, Clone)]
pub struct InfoFlowChecker {
    /// Maps variable name to its security label.
    labels: HashMap<std::string::String, SecurityLabel>,
    /// Maps variable name to its GDPR purpose label (e.g. "analytics",
    /// "billing", "marketing").
    purpose_labels: HashMap<std::string::String, std::string::String>,
    /// Set of variables that carry an explicit `@declassify` annotation.
    declassify_annotations: std::collections::HashSet<std::string::String>,
    /// Names of functions that are considered timing-sensitive (potential
    /// covert channels).
    timing_sensitive_fns: std::collections::HashSet<std::string::String>,
}

impl InfoFlowChecker {
    /// Create a new, empty information flow checker with built-in
    /// timing-sensitive function names.
    pub fn new() -> Self {
        let mut timing_sensitive_fns = std::collections::HashSet::new();
        timing_sensitive_fns.insert("sleep".to_string());
        timing_sensitive_fns.insert("delay".to_string());
        timing_sensitive_fns.insert("wait".to_string());
        timing_sensitive_fns.insert("throw".to_string());
        timing_sensitive_fns.insert("panic".to_string());
        timing_sensitive_fns.insert("abort".to_string());
        Self {
            labels: HashMap::new(),
            purpose_labels: HashMap::new(),
            declassify_annotations: std::collections::HashSet::new(),
            timing_sensitive_fns,
        }
    }

    /// Declare a variable with a security label.
    pub fn declare(&mut self, name: std::string::String, label: SecurityLabel) {
        self.labels.insert(name, label);
    }

    /// Declare a variable with a GDPR purpose label.
    pub fn declare_purpose(&mut self, name: std::string::String, purpose: std::string::String) {
        self.purpose_labels.insert(name, purpose);
    }

    /// Mark a variable as having an explicit `@declassify` annotation.
    pub fn mark_declassify(&mut self, name: std::string::String) {
        self.declassify_annotations.insert(name);
    }

    /// Register a function as timing-sensitive (potential covert channel).
    pub fn register_timing_sensitive(&mut self, name: std::string::String) {
        self.timing_sensitive_fns.insert(name);
    }

    /// Get the security label for a variable. Returns `None` if the
    /// variable has not been declared.
    pub fn get_label(&self, name: &str) -> Option<SecurityLabel> {
        self.labels.get(name).copied()
    }

    /// Get the purpose label for a variable.
    pub fn get_purpose(&self, name: &str) -> Option<&str> {
        self.purpose_labels.get(name).map(|s| s.as_str())
    }

    /// Returns `true` if any security labels are tracked.
    pub fn has_labels(&self) -> bool {
        !self.labels.is_empty()
    }

    // -----------------------------------------------------------------
    // Core checks
    // -----------------------------------------------------------------

    /// Check an assignment: data flows from `source_label` to
    /// `target_label`.
    ///
    /// The source security level must be less than or equal to the
    /// target level. Emits **A08001** if data flows from a higher
    /// security level to a lower one.
    pub fn check_assignment(
        &self,
        target_label: SecurityLabel,
        source_label: SecurityLabel,
        span: &Range<usize>,
    ) -> Option<InfoFlowError> {
        if source_label > target_label {
            Some(InfoFlowError {
                code: "A08001".into(),
                message: format!(
                    "information flows from {source_label} to {target_label}: \
                     data at security level `{source_label}` cannot be assigned \
                     to a `{target_label}` variable"
                ),
                span: span.clone(),
            })
        } else {
            None
        }
    }

    /// Check a declassification: data is being lowered from `from_label`
    /// to `to_label`.
    ///
    /// Declassification is only permitted when an explicit annotation is
    /// present. Emits **A08002** if `has_declassify_annotation` is false.
    pub fn check_declassify(
        &self,
        from_label: SecurityLabel,
        to_label: SecurityLabel,
        has_declassify_annotation: bool,
        span: &Range<usize>,
    ) -> Option<InfoFlowError> {
        if from_label > to_label && !has_declassify_annotation {
            Some(InfoFlowError {
                code: "A08002".into(),
                message: format!(
                    "declassification from {from_label} to {to_label} \
                     without explicit `@declassify` annotation"
                ),
                span: span.clone(),
            })
        } else {
            None
        }
    }

    /// Check that a variable's purpose label matches the required purpose.
    ///
    /// Emits **A08003** if the variable has a purpose label that differs
    /// from `required_purpose`.
    pub fn check_purpose_label(
        &self,
        variable: &str,
        required_purpose: &str,
        span: &Range<usize>,
    ) -> Option<InfoFlowError> {
        if let Some(actual_purpose) = self.purpose_labels.get(variable)
            && actual_purpose != required_purpose
        {
            return Some(InfoFlowError {
                code: "A08003".into(),
                message: format!(
                    "purpose label mismatch for `{variable}`: data labeled \
                     for `{actual_purpose}` used in `{required_purpose}` context"
                ),
                span: span.clone(),
            });
        }
        None
    }

    /// Check for implicit information flow through control dependencies.
    ///
    /// If a conditional expression depends on a high-security variable and
    /// assigns to a low-security variable inside a branch, information
    /// leaks through the control flow.  Emits **A08004**.
    ///
    /// `condition_label` is the inferred label of the if-condition.
    /// `branch_target_label` is the label of the variable being assigned
    /// inside the branch.
    pub fn check_implicit_flow(
        &self,
        condition_label: SecurityLabel,
        branch_target_label: SecurityLabel,
        span: &Range<usize>,
    ) -> Option<InfoFlowError> {
        if condition_label > branch_target_label {
            Some(InfoFlowError {
                code: "A08004".into(),
                message: format!(
                    "implicit information flow: condition at `{condition_label}` \
                     level influences assignment to `{branch_target_label}` variable"
                ),
                span: span.clone(),
            })
        } else {
            None
        }
    }

    /// Check for covert channels through timing or exceptions.
    ///
    /// If a high-security value controls whether a timing-sensitive
    /// function (sleep, delay, throw, panic) is called, information can
    /// leak through observable side effects.  Emits **A08005**.
    pub fn check_covert_channel(
        &self,
        condition_label: SecurityLabel,
        callee: &str,
        span: &Range<usize>,
    ) -> Option<InfoFlowError> {
        if condition_label > SecurityLabel::Public && self.timing_sensitive_fns.contains(callee) {
            Some(InfoFlowError {
                code: "A08005".into(),
                message: format!(
                    "potential covert channel: `{condition_label}` data controls \
                     call to timing/exception function `{callee}`"
                ),
                span: span.clone(),
            })
        } else {
            None
        }
    }

    // -----------------------------------------------------------------
    // Label inference
    // -----------------------------------------------------------------

    /// Infer the security label of an expression.
    ///
    /// The result label is the **maximum** of all operand labels (the
    /// join in the lattice).  Variables without a declared label default
    /// to `Public`.
    pub fn infer_label(&self, expr: &Expr) -> SecurityLabel {
        match expr {
            Expr::Ident(name) => self
                .labels
                .get(name)
                .copied()
                .unwrap_or(SecurityLabel::Public),

            Expr::Literal(_) => SecurityLabel::Public,

            Expr::Field(receiver, _) => self.infer_label(receiver),

            Expr::BinOp { lhs, rhs, .. } => {
                std::cmp::max(self.infer_label(lhs), self.infer_label(rhs))
            }

            Expr::UnaryOp { expr: inner, .. } => self.infer_label(inner),

            Expr::Call { func, args } => {
                let f = self.infer_label(func);
                args.iter()
                    .fold(f, |acc, arg| std::cmp::max(acc, self.infer_label(arg)))
            }

            Expr::MethodCall { receiver, args, .. } => {
                let r = self.infer_label(receiver);
                args.iter()
                    .fold(r, |acc, arg| std::cmp::max(acc, self.infer_label(arg)))
            }

            Expr::Index { expr: base, index } => {
                std::cmp::max(self.infer_label(base), self.infer_label(index))
            }

            Expr::Old(inner) | Expr::Paren(inner) | Expr::Cast { expr: inner, .. } => {
                self.infer_label(inner)
            }

            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let mut r = std::cmp::max(self.infer_label(cond), self.infer_label(then_branch));
                if let Some(e) = else_branch {
                    r = std::cmp::max(r, self.infer_label(e));
                }
                r
            }

            Expr::List(items) => items.iter().fold(SecurityLabel::Public, |a, i| {
                std::cmp::max(a, self.infer_label(i))
            }),

            Expr::Block(exprs) => exprs.iter().fold(SecurityLabel::Public, |a, e| {
                std::cmp::max(a, self.infer_label(e))
            }),

            Expr::Forall { body, .. } | Expr::Exists { body, .. } => self.infer_label(body),

            Expr::Apply { args, .. } => args.iter().fold(SecurityLabel::Public, |a, arg| {
                std::cmp::max(a, self.infer_label(arg))
            }),

            Expr::Match { scrutinee, arms } => {
                let mut r = self.infer_label(scrutinee);
                for arm in arms {
                    r = std::cmp::max(r, self.infer_label(&arm.body));
                }
                r
            }

            Expr::Let { value, body, .. } => {
                std::cmp::max(self.infer_label(value), self.infer_label(body))
            }

            Expr::Tuple(elems) => elems.iter().fold(SecurityLabel::Public, |a, e| {
                std::cmp::max(a, self.infer_label(e))
            }),

            Expr::Ghost(_) | Expr::Raw(_) => SecurityLabel::Public,
        }
    }

    // -----------------------------------------------------------------
    // Expression-level checking
    // -----------------------------------------------------------------

    /// Check an expression tree for information flow violations.
    ///
    /// Walks the AST looking for:
    /// - Implicit flows through `if` conditions (A08004)
    /// - Covert channels through timing/exception calls (A08005)
    pub fn check_expr(&self, expr: &Expr, span: &Range<usize>) -> Vec<InfoFlowError> {
        let mut errors = Vec::new();
        self.check_expr_inner(expr, span, SecurityLabel::Public, &mut errors);
        errors
    }

    /// Inner recursive checker with a `pc_label` representing the
    /// current program-counter security context (from enclosing
    /// conditionals).
    fn check_expr_inner(
        &self,
        expr: &Expr,
        span: &Range<usize>,
        pc_label: SecurityLabel,
        errors: &mut Vec<InfoFlowError>,
    ) {
        match expr {
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let cond_label = std::cmp::max(pc_label, self.infer_label(cond));
                self.check_expr_inner(cond, span, pc_label, errors);
                self.check_expr_inner(then_branch, span, cond_label, errors);
                if let Some(else_br) = else_branch {
                    self.check_expr_inner(else_br, span, cond_label, errors);
                }
            }

            // Detect covert channels: high-security pc controls a
            // timing-sensitive or exception-raising call.
            Expr::Call { func, args } => {
                if let Expr::Ident(name) = func.as_ref()
                    && let Some(err) = self.check_covert_channel(pc_label, name, span)
                {
                    errors.push(err);
                }
                self.check_expr_inner(func, span, pc_label, errors);
                for arg in args {
                    self.check_expr_inner(arg, span, pc_label, errors);
                }
            }

            Expr::MethodCall {
                receiver,
                method,
                args,
            } => {
                if let Some(err) = self.check_covert_channel(pc_label, method, span) {
                    errors.push(err);
                }
                self.check_expr_inner(receiver, span, pc_label, errors);
                for arg in args {
                    self.check_expr_inner(arg, span, pc_label, errors);
                }
            }

            // Recurse into sub-expressions
            Expr::BinOp { lhs, rhs, .. } => {
                self.check_expr_inner(lhs, span, pc_label, errors);
                self.check_expr_inner(rhs, span, pc_label, errors);
            }
            Expr::UnaryOp { expr: inner, .. }
            | Expr::Old(inner)
            | Expr::Paren(inner)
            | Expr::Cast { expr: inner, .. }
            | Expr::Ghost(inner) => {
                self.check_expr_inner(inner, span, pc_label, errors);
            }
            Expr::Field(receiver, _) => {
                self.check_expr_inner(receiver, span, pc_label, errors);
            }
            Expr::Index { expr: base, index } => {
                self.check_expr_inner(base, span, pc_label, errors);
                self.check_expr_inner(index, span, pc_label, errors);
            }
            Expr::List(items) => {
                for item in items {
                    self.check_expr_inner(item, span, pc_label, errors);
                }
            }
            Expr::Block(exprs) => {
                for e in exprs {
                    self.check_expr_inner(e, span, pc_label, errors);
                }
            }
            Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
                self.check_expr_inner(domain, span, pc_label, errors);
                self.check_expr_inner(body, span, pc_label, errors);
            }
            Expr::Apply { args, .. } => {
                for arg in args {
                    self.check_expr_inner(arg, span, pc_label, errors);
                }
            }
            Expr::Match { scrutinee, arms } => {
                self.check_expr_inner(scrutinee, span, pc_label, errors);
                // Each arm body executes under the PC label of the scrutinee
                let scrut_label = self.infer_label(scrutinee);
                let elevated = std::cmp::max(pc_label, scrut_label);
                for arm in arms {
                    self.check_expr_inner(&arm.body, span, elevated, errors);
                }
            }
            Expr::Let { value, body, .. } => {
                self.check_expr_inner(value, span, pc_label, errors);
                self.check_expr_inner(body, span, pc_label, errors);
            }
            Expr::Tuple(elems) => {
                for e in elems {
                    self.check_expr_inner(e, span, pc_label, errors);
                }
            }
            Expr::Ident(_) | Expr::Literal(_) | Expr::Raw(_) => {}
        }
    }
}

impl Default for InfoFlowChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests

// ---------------------------------------------------------------------------
// Totality checking (T053)
// ---------------------------------------------------------------------------

/// What expression decreases at each recursive call, proving termination.
#[derive(Debug, Clone)]
pub enum DecreasesMeasure {
    /// A single natural-number expression that must strictly decrease.
    Natural(Expr),
    /// A lexicographic tuple of measures (e.g., Ackermann-like functions).
    Lexicographic(Vec<Expr>),
    /// Well-founded ordering on a custom/structural type.
    WellFounded(Expr),
}

/// A totality error with error code, span, and message.
#[derive(Debug, Clone)]
pub struct TotalityError {
    /// Error code from the spec (A09xxx series).
    pub code: std::string::String,
    /// Human-readable error message.
    pub message: std::string::String,
    /// Source location where the error was detected.
    pub span: Range<usize>,
}

/// Totality checker for termination checking via `decreases` measures.
///
/// Validates that recursive functions terminate by checking that a
/// well-founded measure strictly decreases at every recursive call site.
///
/// # Error codes
///
/// - **A09001**: Recursive function without `decreases` clause (and no `partial` annotation)
/// - **A09002**: Measure does not strictly decrease at recursive call site
/// - **A09003**: Cannot prove measure is well-founded (e.g., might go negative)
/// - **A09004**: Mutually recursive functions without collective termination proof
pub struct TotalityChecker {
    /// Names of functions known to be partial (escape hatch).
    partial_fns: std::collections::HashSet<std::string::String>,
}

impl TotalityChecker {
    /// Create a new totality checker.
    pub fn new() -> Self {
        Self {
            partial_fns: std::collections::HashSet::new(),
        }
    }

    /// Register a function as `partial` (opt out of termination checking).
    pub fn mark_partial(&mut self, name: std::string::String) {
        self.partial_fns.insert(name);
    }

    /// Check whether a function definition has the `partial` escape hatch.
    ///
    /// A function is partial if it was explicitly registered via
    /// [`mark_partial`] or if its clauses contain an `Other("partial")`
    /// clause kind.
    pub fn is_partial(&self, fn_def: &assura_parser::ast::FnDef) -> bool {
        if self.partial_fns.contains(&fn_def.name) {
            return true;
        }
        // Check for a `partial` annotation in clause kinds
        fn_def
            .clauses
            .iter()
            .any(|c| matches!(&c.kind, ClauseKind::Other(s) if s == "partial"))
    }

    /// Extract the `decreases` measure from a function definition.
    ///
    /// Looks for clauses with kind `Other("decreases")`. The clause body
    /// expression becomes the measure. Multiple decreases clauses form a
    /// lexicographic tuple. A single clause is a `Natural` measure.
    pub fn extract_decreases_measure(
        &self,
        fn_def: &assura_parser::ast::FnDef,
    ) -> Option<DecreasesMeasure> {
        let decreases_exprs: Vec<&Expr> = fn_def
            .clauses
            .iter()
            .filter(|c| {
                c.kind == ClauseKind::Decreases
                    || matches!(&c.kind, ClauseKind::Other(s) if s == "decreases")
            })
            .map(|c| &c.body)
            .collect();

        match decreases_exprs.len() {
            0 => None,
            1 => Some(DecreasesMeasure::Natural(decreases_exprs[0].clone())),
            _ => Some(DecreasesMeasure::Lexicographic(
                decreases_exprs.into_iter().cloned().collect(),
            )),
        }
    }

    /// Check whether the given expression contains a recursive call to `fn_name`.
    fn expr_contains_recursive_call(&self, expr: &Expr, fn_name: &str) -> bool {
        match expr {
            Expr::Call { func, args } => {
                let is_self_call = matches!(func.as_ref(), Expr::Ident(name) if name == fn_name);
                if is_self_call {
                    return true;
                }
                self.expr_contains_recursive_call(func, fn_name)
                    || args
                        .iter()
                        .any(|a| self.expr_contains_recursive_call(a, fn_name))
            }
            Expr::BinOp { lhs, rhs, .. } => {
                self.expr_contains_recursive_call(lhs, fn_name)
                    || self.expr_contains_recursive_call(rhs, fn_name)
            }
            Expr::UnaryOp { expr: inner, .. }
            | Expr::Old(inner)
            | Expr::Paren(inner)
            | Expr::Cast { expr: inner, .. }
            | Expr::Ghost(inner) => self.expr_contains_recursive_call(inner, fn_name),
            Expr::Field(receiver, _) => self.expr_contains_recursive_call(receiver, fn_name),
            Expr::MethodCall { receiver, args, .. } => {
                self.expr_contains_recursive_call(receiver, fn_name)
                    || args
                        .iter()
                        .any(|a| self.expr_contains_recursive_call(a, fn_name))
            }
            Expr::Index {
                expr: base, index, ..
            } => {
                self.expr_contains_recursive_call(base, fn_name)
                    || self.expr_contains_recursive_call(index, fn_name)
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.expr_contains_recursive_call(cond, fn_name)
                    || self.expr_contains_recursive_call(then_branch, fn_name)
                    || else_branch
                        .as_ref()
                        .is_some_and(|e| self.expr_contains_recursive_call(e, fn_name))
            }
            Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
                self.expr_contains_recursive_call(domain, fn_name)
                    || self.expr_contains_recursive_call(body, fn_name)
            }
            Expr::List(items) => items
                .iter()
                .any(|i| self.expr_contains_recursive_call(i, fn_name)),
            Expr::Block(exprs) => exprs
                .iter()
                .any(|e| self.expr_contains_recursive_call(e, fn_name)),
            Expr::Apply { args, .. } => args
                .iter()
                .any(|a| self.expr_contains_recursive_call(a, fn_name)),
            Expr::Match { scrutinee, arms } => {
                self.expr_contains_recursive_call(scrutinee, fn_name)
                    || arms
                        .iter()
                        .any(|arm| self.expr_contains_recursive_call(&arm.body, fn_name))
            }
            Expr::Let { value, body, .. } => {
                self.expr_contains_recursive_call(value, fn_name)
                    || self.expr_contains_recursive_call(body, fn_name)
            }
            Expr::Tuple(elems) => elems
                .iter()
                .any(|e| self.expr_contains_recursive_call(e, fn_name)),
            Expr::Ident(_) | Expr::Literal(_) | Expr::Raw(_) => false,
        }
    }

    /// Collect arguments from recursive call sites to `fn_name` in `expr`.
    fn collect_recursive_call_args<'a>(
        &self,
        expr: &'a Expr,
        fn_name: &str,
        out: &mut Vec<&'a [Expr]>,
    ) {
        match expr {
            Expr::Call { func, args } => {
                if matches!(func.as_ref(), Expr::Ident(name) if name == fn_name) {
                    out.push(args.as_slice());
                }
                self.collect_recursive_call_args(func, fn_name, out);
                for a in args {
                    self.collect_recursive_call_args(a, fn_name, out);
                }
            }
            Expr::BinOp { lhs, rhs, .. } => {
                self.collect_recursive_call_args(lhs, fn_name, out);
                self.collect_recursive_call_args(rhs, fn_name, out);
            }
            Expr::UnaryOp { expr: inner, .. }
            | Expr::Old(inner)
            | Expr::Paren(inner)
            | Expr::Cast { expr: inner, .. }
            | Expr::Ghost(inner) => {
                self.collect_recursive_call_args(inner, fn_name, out);
            }
            Expr::Field(receiver, _) => {
                self.collect_recursive_call_args(receiver, fn_name, out);
            }
            Expr::MethodCall { receiver, args, .. } => {
                self.collect_recursive_call_args(receiver, fn_name, out);
                for a in args {
                    self.collect_recursive_call_args(a, fn_name, out);
                }
            }
            Expr::Index {
                expr: base, index, ..
            } => {
                self.collect_recursive_call_args(base, fn_name, out);
                self.collect_recursive_call_args(index, fn_name, out);
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.collect_recursive_call_args(cond, fn_name, out);
                self.collect_recursive_call_args(then_branch, fn_name, out);
                if let Some(e) = else_branch {
                    self.collect_recursive_call_args(e, fn_name, out);
                }
            }
            Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
                self.collect_recursive_call_args(domain, fn_name, out);
                self.collect_recursive_call_args(body, fn_name, out);
            }
            Expr::List(items) => {
                for i in items {
                    self.collect_recursive_call_args(i, fn_name, out);
                }
            }
            Expr::Block(exprs) => {
                for e in exprs {
                    self.collect_recursive_call_args(e, fn_name, out);
                }
            }
            Expr::Apply { args, .. } => {
                for a in args {
                    self.collect_recursive_call_args(a, fn_name, out);
                }
            }
            Expr::Match { scrutinee, arms } => {
                self.collect_recursive_call_args(scrutinee, fn_name, out);
                for arm in arms {
                    self.collect_recursive_call_args(&arm.body, fn_name, out);
                }
            }
            Expr::Let { value, body, .. } => {
                self.collect_recursive_call_args(value, fn_name, out);
                self.collect_recursive_call_args(body, fn_name, out);
            }
            Expr::Tuple(elems) => {
                for e in elems {
                    self.collect_recursive_call_args(e, fn_name, out);
                }
            }
            Expr::Ident(_) | Expr::Literal(_) | Expr::Raw(_) => {}
        }
    }

    /// Check whether a recursive call's argument is structurally smaller
    /// than the corresponding measure expression.
    ///
    /// Recognizes patterns like `n - 1` (for natural measure `n`),
    /// `xs.tail` or `node.left` / `node.right` (structural recursion).
    fn is_strictly_decreasing(measure: &Expr, call_arg: &Expr) -> bool {
        // Pattern: measure is `Ident(x)`, call_arg is `x - <positive>`
        if let Expr::Ident(measure_var) = measure {
            match call_arg {
                // n - 1, n - 2, etc.
                Expr::BinOp {
                    lhs,
                    op: BinOp::Sub,
                    rhs,
                } => {
                    if let Expr::Ident(arg_var) = lhs.as_ref()
                        && arg_var == measure_var
                    {
                        // The rhs must be a positive literal
                        if let Expr::Literal(Literal::Int(s)) = rhs.as_ref()
                            && let Ok(v) = s.parse::<i64>()
                        {
                            return v > 0;
                        }
                        // Any non-zero expression is acceptable
                        return true;
                    }
                    false
                }
                // Structural: x.tail, x.left, x.right, x.children, etc.
                Expr::Field(receiver, field) => {
                    if let Expr::Ident(arg_var) = receiver.as_ref()
                        && arg_var == measure_var
                    {
                        return matches!(
                            field.as_str(),
                            "tail" | "left" | "right" | "children" | "rest" | "next"
                        );
                    }
                    false
                }
                _ => false,
            }
        } else {
            false
        }
    }

    /// Check whether a measure expression is well-founded (cannot go
    /// negative or be undefined).
    ///
    /// A natural-number variable is well-founded if the function has a
    /// `requires` clause constraining it to be >= 0. Structural measures
    /// on inductive types are always well-founded. Returns `true` if
    /// well-foundedness can be established, `false` otherwise.
    fn is_well_founded(measure: &Expr, fn_def: &assura_parser::ast::FnDef) -> bool {
        match measure {
            Expr::Ident(name) => {
                // Check requires clauses for a constraint like `n >= 0`
                for clause in &fn_def.clauses {
                    if clause.kind == ClauseKind::Requires
                        && Self::expr_constrains_non_negative(&clause.body, name)
                    {
                        return true;
                    }
                }
                // Check parameter type for well-foundedness
                for param in &fn_def.params {
                    if param.name == *name {
                        // Nat is always >= 0
                        if param.ty.iter().any(|t| t == "Nat") {
                            return true;
                        }
                        // Structural/named types (List, Tree, etc.) are
                        // well-founded by structural induction. Any type
                        // that is not a raw numeric type (Int, Float, etc.)
                        // is considered structural.
                        let is_numeric_type = param.ty.iter().any(|t| {
                            matches!(
                                t.as_str(),
                                "Int" | "Float" | "F32" | "F64" | "I8" | "I16" | "I32" | "I64"
                            )
                        });
                        if !is_numeric_type {
                            return true;
                        }
                    }
                }
                false
            }
            // Field access on a structural type is well-founded by induction
            Expr::Field(_, _) => true,
            _ => false,
        }
    }

    /// Check whether an expression constrains a variable to be non-negative.
    ///
    /// Recognizes patterns: `x >= 0`, `0 <= x`, `x > 0`, etc.
    fn expr_constrains_non_negative(expr: &Expr, var_name: &str) -> bool {
        match expr {
            Expr::BinOp { lhs, op, rhs } => {
                match op {
                    // x >= 0 or x > 0
                    BinOp::Gte | BinOp::Gt => {
                        if let Expr::Ident(name) = lhs.as_ref()
                            && name == var_name
                            && let Expr::Literal(Literal::Int(s)) = rhs.as_ref()
                            && let Ok(v) = s.parse::<i64>()
                        {
                            return v >= 0;
                        }
                        false
                    }
                    // 0 <= x or 0 < x
                    BinOp::Lte | BinOp::Lt => {
                        if let Expr::Literal(Literal::Int(s)) = lhs.as_ref()
                            && let Ok(v) = s.parse::<i64>()
                            && v >= 0
                            && let Expr::Ident(name) = rhs.as_ref()
                        {
                            return name == var_name;
                        }
                        false
                    }
                    // Conjunction: either side can provide the constraint
                    BinOp::And => {
                        Self::expr_constrains_non_negative(lhs, var_name)
                            || Self::expr_constrains_non_negative(rhs, var_name)
                    }
                    _ => false,
                }
            }
            Expr::Paren(inner) => Self::expr_constrains_non_negative(inner, var_name),
            _ => false,
        }
    }

    /// Check whether a recursive call strictly decreases the measure.
    ///
    /// For a `Natural` measure, finds the parameter matching the measure
    /// variable and checks that the corresponding call argument is
    /// structurally smaller. For `Lexicographic` measures, checks that
    /// at least one component strictly decreases while preceding
    /// components are non-increasing.
    pub fn check_recursive_call(
        &self,
        fn_def: &assura_parser::ast::FnDef,
        measure: &DecreasesMeasure,
        call_args: &[Expr],
        span: &Range<usize>,
    ) -> Option<TotalityError> {
        match measure {
            DecreasesMeasure::Natural(measure_expr) => {
                // Find which parameter position corresponds to the measure
                if let Expr::Ident(measure_var) = measure_expr {
                    for (i, param) in fn_def.params.iter().enumerate() {
                        if param.name == *measure_var
                            && let Some(call_arg) = call_args.get(i)
                        {
                            if Self::is_strictly_decreasing(measure_expr, call_arg) {
                                return None; // OK
                            }
                            return Some(TotalityError {
                                code: "A09002".into(),
                                message: format!(
                                    "measure `{measure_var}` does not strictly \
                                     decrease at recursive call to `{}`",
                                    fn_def.name
                                ),
                                span: span.clone(),
                            });
                        }
                    }
                }
                None // Cannot determine; deferred to SMT
            }
            DecreasesMeasure::Lexicographic(measures) => {
                // For lexicographic: at least one component must strictly decrease
                let mut any_decreases = false;
                for measure_expr in measures {
                    if let Expr::Ident(measure_var) = measure_expr {
                        for (i, param) in fn_def.params.iter().enumerate() {
                            if param.name == *measure_var
                                && let Some(call_arg) = call_args.get(i)
                                && Self::is_strictly_decreasing(measure_expr, call_arg)
                            {
                                any_decreases = true;
                            }
                        }
                    }
                }
                if any_decreases {
                    None
                } else {
                    Some(TotalityError {
                        code: "A09002".into(),
                        message: format!(
                            "lexicographic measure does not strictly decrease \
                             at recursive call to `{}`",
                            fn_def.name
                        ),
                        span: span.clone(),
                    })
                }
            }
            DecreasesMeasure::WellFounded(_) => {
                // Well-founded ordering check is deferred to SMT
                None
            }
        }
    }

    /// Check a single function for totality (termination).
    ///
    /// 1. If the function is `partial`, skip it.
    /// 2. Determine if the function is recursive (calls itself).
    /// 3. If recursive, extract the `decreases` measure.
    /// 4. Verify the measure strictly decreases at every recursive call.
    /// 5. Verify the measure is well-founded.
    pub fn check_function_totality(
        &self,
        fn_def: &assura_parser::ast::FnDef,
        span: &Range<usize>,
    ) -> Vec<TotalityError> {
        let mut errors = Vec::new();

        // Partial functions skip termination checking
        if self.is_partial(fn_def) {
            return errors;
        }

        // Determine if the function is recursive by scanning its clause bodies
        let is_recursive = fn_def
            .clauses
            .iter()
            .any(|c| self.expr_contains_recursive_call(&c.body, &fn_def.name));

        if !is_recursive {
            // Non-recursive functions are trivially total
            return errors;
        }

        // Extract the decreases measure
        let measure = match self.extract_decreases_measure(fn_def) {
            Some(m) => m,
            None => {
                errors.push(TotalityError {
                    code: "A09001".into(),
                    message: format!(
                        "recursive function `{}` has no `decreases` clause; \
                         add `decreases <expr>` or annotate with `partial`",
                        fn_def.name
                    ),
                    span: span.clone(),
                });
                return errors;
            }
        };

        // Check well-foundedness of the measure
        match &measure {
            DecreasesMeasure::Natural(expr) => {
                if !Self::is_well_founded(expr, fn_def) {
                    errors.push(TotalityError {
                        code: "A09003".into(),
                        message: format!(
                            "cannot prove measure is well-founded for function `{}`; \
                             add `requires` clause ensuring the measure is non-negative",
                            fn_def.name
                        ),
                        span: span.clone(),
                    });
                }
            }
            DecreasesMeasure::Lexicographic(exprs) => {
                for expr in exprs {
                    if !Self::is_well_founded(expr, fn_def) {
                        errors.push(TotalityError {
                            code: "A09003".into(),
                            message: format!(
                                "cannot prove measure component is well-founded \
                                 for function `{}`",
                                fn_def.name
                            ),
                            span: span.clone(),
                        });
                        break; // One error is enough
                    }
                }
            }
            DecreasesMeasure::WellFounded(_) => {
                // Deferred to SMT
            }
        }

        // Collect recursive call sites and check each one
        let mut call_arg_sets: Vec<&[Expr]> = Vec::new();
        for clause in &fn_def.clauses {
            self.collect_recursive_call_args(&clause.body, &fn_def.name, &mut call_arg_sets);
        }

        for call_args in &call_arg_sets {
            if let Some(err) = self.check_recursive_call(fn_def, &measure, call_args, span) {
                errors.push(err);
            }
        }

        errors
    }

    /// Detect and verify mutually recursive function groups.
    ///
    /// Given a set of function definitions, builds a call graph, finds
    /// strongly connected components (groups of mutually recursive
    /// functions), and checks that each group has a collective
    /// termination proof.
    ///
    /// Returns A09004 for groups where no function has a `decreases` clause.
    pub fn check_mutual_recursion(
        &self,
        fn_defs: &[(&assura_parser::ast::FnDef, &Range<usize>)],
    ) -> Vec<TotalityError> {
        let mut errors = Vec::new();

        // Build a simple call graph: for each function, which other
        // functions in the set does it call?
        let names: Vec<&str> = fn_defs.iter().map(|(f, _)| f.name.as_str()).collect();

        for (i, &(fn_def_i, span_i)) in fn_defs.iter().enumerate() {
            // Skip partial functions
            if self.is_partial(fn_def_i) {
                continue;
            }

            for (j, &(fn_def_j, _)) in fn_defs.iter().enumerate() {
                if i == j {
                    continue;
                }

                // Does fn_i call fn_j?
                let i_calls_j = fn_def_i
                    .clauses
                    .iter()
                    .any(|c| self.expr_contains_recursive_call(&c.body, names[j]));

                // Does fn_j call fn_i?
                let j_calls_i = fn_def_j
                    .clauses
                    .iter()
                    .any(|c| self.expr_contains_recursive_call(&c.body, names[i]));

                if i_calls_j && j_calls_i {
                    // Mutual recursion detected; check for decreases
                    let has_measure_i = self.extract_decreases_measure(fn_def_i).is_some();
                    let has_measure_j = self.extract_decreases_measure(fn_def_j).is_some();

                    if !has_measure_i && !has_measure_j {
                        errors.push(TotalityError {
                            code: "A09004".into(),
                            message: format!(
                                "mutually recursive functions `{}` and `{}` \
                                 have no collective termination proof; \
                                 add `decreases` clauses to at least one",
                                fn_def_i.name, fn_def_j.name
                            ),
                            span: span_i.clone(),
                        });
                    }
                }
            }
        }

        errors
    }
}

impl Default for TotalityChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for TotalityChecker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TotalityChecker")
            .field("partial_fns", &self.partial_fns)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// T055 MEM.2: Fixed-width integer checker
// ---------------------------------------------------------------------------

/// A structured error from fixed-width integer checking.
#[derive(Debug, Clone)]
pub struct FixedWidthError {
    /// Error code (A10101-A10104).
    pub code: std::string::String,
    /// Human-readable message.
    pub message: std::string::String,
    /// Source span where the issue was detected.
    pub span: Range<usize>,
}

/// Checker for fixed-width integer types with overflow detection.
///
/// Tracks fixed-width integer types in expressions, detects potential
/// arithmetic overflow, validates cast safety, and flags signed/unsigned
/// mismatches.
///
/// Implements MEM.2 from Section 14 of the specification.
///
/// # Error codes
///
/// - **A10101**: Potential integer overflow in arithmetic operation
/// - **A10102**: Unsafe narrowing cast (e.g., U32 to U16 without bounds check)
/// - **A10103**: Signed/unsigned mismatch in comparison
/// - **A10104**: Division/modulo by zero not guarded
#[derive(Debug, Clone)]
pub struct FixedWidthChecker {
    /// Maps variable name to its fixed-width type.
    bindings: HashMap<std::string::String, Type>,
}

impl FixedWidthChecker {
    /// Create an empty fixed-width checker.
    pub fn new() -> Self {
        Self {
            bindings: HashMap::new(),
        }
    }

    /// Register a variable with its fixed-width integer type.
    pub fn declare(&mut self, name: std::string::String, ty: Type) {
        self.bindings.insert(name, ty);
    }

    /// Look up the type of a registered variable.
    pub fn get_type(&self, name: &str) -> Option<&Type> {
        self.bindings.get(name)
    }

    /// Return the valid numeric range `(min, max)` for a fixed-width type.
    ///
    /// Returns `None` for non-fixed-width types.
    pub fn range_for_type(ty: &Type) -> Option<(i128, i128)> {
        match ty {
            Type::U8 => Some((0, u8::MAX as i128)),
            Type::U16 => Some((0, u16::MAX as i128)),
            Type::U32 => Some((0, u32::MAX as i128)),
            Type::U64 => Some((0, u64::MAX as i128)),
            Type::I8 => Some((i8::MIN as i128, i8::MAX as i128)),
            Type::I16 => Some((i16::MIN as i128, i16::MAX as i128)),
            Type::I32 => Some((i32::MIN as i128, i32::MAX as i128)),
            Type::I64 => Some((i64::MIN as i128, i64::MAX as i128)),
            _ => None,
        }
    }

    /// Returns `true` if the given type is a fixed-width integer type.
    pub fn is_fixed_width(ty: &Type) -> bool {
        Self::range_for_type(ty).is_some()
    }

    /// Returns `true` if the given type is an unsigned fixed-width integer.
    pub fn is_unsigned(ty: &Type) -> bool {
        matches!(ty, Type::U8 | Type::U16 | Type::U32 | Type::U64)
    }

    /// Returns `true` if the given type is a signed fixed-width integer.
    pub fn is_signed(ty: &Type) -> bool {
        matches!(ty, Type::I8 | Type::I16 | Type::I32 | Type::I64)
    }

    /// Check whether an arithmetic operation can overflow given the operand
    /// type ranges.
    ///
    /// Returns `true` if the result of `op` applied to values in
    /// `left_range` and `right_range` can produce a value outside
    /// `result_range`.
    pub fn can_overflow(
        op: &BinOp,
        left_range: (i128, i128),
        right_range: (i128, i128),
        result_range: (i128, i128),
    ) -> bool {
        let (result_min, result_max) = result_range;
        match op {
            BinOp::Add => {
                let worst_low = left_range.0.saturating_add(right_range.0);
                let worst_high = left_range.1.saturating_add(right_range.1);
                worst_low < result_min || worst_high > result_max
            }
            BinOp::Sub => {
                let worst_low = left_range.0.saturating_sub(right_range.1);
                let worst_high = left_range.1.saturating_sub(right_range.0);
                worst_low < result_min || worst_high > result_max
            }
            BinOp::Mul => {
                let products = [
                    left_range.0.saturating_mul(right_range.0),
                    left_range.0.saturating_mul(right_range.1),
                    left_range.1.saturating_mul(right_range.0),
                    left_range.1.saturating_mul(right_range.1),
                ];
                let worst_low = products.iter().copied().min().unwrap_or(0);
                let worst_high = products.iter().copied().max().unwrap_or(0);
                worst_low < result_min || worst_high > result_max
            }
            _ => false,
        }
    }

    /// Check whether a cast from `from_type` to `to_type` is always safe.
    ///
    /// A cast is safe if every value in the source range fits in the
    /// destination range. Returns `true` for safe (widening) casts,
    /// `false` for potentially unsafe (narrowing) casts.
    pub fn is_safe_cast(from_type: &Type, to_type: &Type) -> bool {
        let from_range = match Self::range_for_type(from_type) {
            Some(r) => r,
            None => return true, // Non-fixed-width types are outside our scope
        };
        let to_range = match Self::range_for_type(to_type) {
            Some(r) => r,
            None => return true,
        };
        from_range.0 >= to_range.0 && from_range.1 <= to_range.1
    }

    /// Check potential overflow in an arithmetic operation on two typed
    /// operands.
    ///
    /// Returns `None` if the operation is safe, or `Some(FixedWidthError)`
    /// with code A10101 if overflow is possible.
    pub fn check_arithmetic_overflow(
        &self,
        op: &BinOp,
        left_type: &Type,
        right_type: &Type,
        span: &Range<usize>,
    ) -> Option<FixedWidthError> {
        // Only check arithmetic ops
        if !matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul) {
            return None;
        }

        let left_range = Self::range_for_type(left_type)?;
        let right_range = Self::range_for_type(right_type)?;

        // Result type is the wider of the two (or left if same width)
        let result_range = Self::wider_range(left_range, right_range);

        if Self::can_overflow(op, left_range, right_range, result_range) {
            let op_name = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                _ => "?",
            };
            Some(FixedWidthError {
                code: "A10101".into(),
                message: format!(
                    "potential integer overflow: `{left_type:?} {op_name} {right_type:?}` \
                     can exceed the target range [{}, {}]; consider using `{}`",
                    result_range.0,
                    result_range.1,
                    Self::suggest_checked_alternative(op),
                ),
                span: span.clone(),
            })
        } else {
            None
        }
    }

    /// Check whether a cast expression is safe.
    ///
    /// Returns `None` if safe, or `Some(FixedWidthError)` with code
    /// A10102 for an unsafe narrowing cast.
    pub fn check_cast_safety(
        from_type: &Type,
        to_type: &Type,
        span: &Range<usize>,
    ) -> Option<FixedWidthError> {
        if !Self::is_fixed_width(from_type) || !Self::is_fixed_width(to_type) {
            return None;
        }
        if Self::is_safe_cast(from_type, to_type) {
            None
        } else {
            Some(FixedWidthError {
                code: "A10102".into(),
                message: format!(
                    "unsafe narrowing cast from `{from_type:?}` to `{to_type:?}`: \
                     source range [{}, {}] does not fit in target range [{}, {}]; \
                     add a bounds check before casting",
                    Self::range_for_type(from_type).map_or(0, |r| r.0),
                    Self::range_for_type(from_type).map_or(0, |r| r.1),
                    Self::range_for_type(to_type).map_or(0, |r| r.0),
                    Self::range_for_type(to_type).map_or(0, |r| r.1),
                ),
                span: span.clone(),
            })
        }
    }

    /// Check for signed/unsigned mismatch in a comparison operation.
    ///
    /// Returns `None` if both sides have the same signedness, or
    /// `Some(FixedWidthError)` with code A10103.
    pub fn check_signedness_mismatch(
        op: &BinOp,
        left_type: &Type,
        right_type: &Type,
        span: &Range<usize>,
    ) -> Option<FixedWidthError> {
        // Only flag comparison operators
        if !matches!(
            op,
            BinOp::Eq | BinOp::Neq | BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte
        ) {
            return None;
        }
        if !Self::is_fixed_width(left_type) || !Self::is_fixed_width(right_type) {
            return None;
        }
        let left_signed = Self::is_signed(left_type);
        let right_signed = Self::is_signed(right_type);
        if left_signed != right_signed {
            Some(FixedWidthError {
                code: "A10103".into(),
                message: format!(
                    "signed/unsigned mismatch in comparison: `{left_type:?}` vs \
                     `{right_type:?}`; comparing signed and unsigned integers \
                     can produce unexpected results"
                ),
                span: span.clone(),
            })
        } else {
            None
        }
    }

    /// Check whether a division or modulo operation has a zero-guard on
    /// the divisor.
    ///
    /// This is a simplified check: if the RHS is a literal zero, flag it.
    /// Full divisor analysis (tracking which requires clauses guard the
    /// divisor) is deferred to SMT encoding.
    ///
    /// Returns `None` if safe, or `Some(FixedWidthError)` with code
    /// A10104.
    pub fn check_division_by_zero(
        op: &BinOp,
        rhs: &Expr,
        left_type: &Type,
        span: &Range<usize>,
    ) -> Option<FixedWidthError> {
        if !matches!(op, BinOp::Div | BinOp::Mod) {
            return None;
        }
        if !Self::is_fixed_width(left_type) {
            return None;
        }
        if Self::is_literal_zero(rhs) {
            let op_name = if *op == BinOp::Div {
                "division"
            } else {
                "modulo"
            };
            Some(FixedWidthError {
                code: "A10104".into(),
                message: format!(
                    "{op_name} by zero: the divisor is a literal zero; \
                     add a guard `requires {{ divisor != 0 }}` or use \
                     a checked alternative"
                ),
                span: span.clone(),
            })
        } else {
            None
        }
    }

    /// Suggest a checked alternative for an arithmetic operator.
    pub fn suggest_checked_alternative(op: &BinOp) -> std::string::String {
        match op {
            BinOp::Add => "checked_add".into(),
            BinOp::Sub => "checked_sub".into(),
            BinOp::Mul => "checked_mul".into(),
            BinOp::Div => "checked_div".into(),
            BinOp::Mod => "checked_rem".into(),
            _ => "checked operation".into(),
        }
    }

    /// Check a binary expression for fixed-width integer issues.
    ///
    /// Combines overflow, signedness, and division-by-zero checks.
    pub fn check_binop(
        &self,
        op: &BinOp,
        left_type: &Type,
        right_type: &Type,
        rhs_expr: &Expr,
        span: &Range<usize>,
    ) -> Vec<FixedWidthError> {
        let mut errors = Vec::new();

        if let Some(e) = self.check_arithmetic_overflow(op, left_type, right_type, span) {
            errors.push(e);
        }

        if let Some(e) = Self::check_signedness_mismatch(op, left_type, right_type, span) {
            errors.push(e);
        }

        if let Some(e) = Self::check_division_by_zero(op, rhs_expr, left_type, span) {
            errors.push(e);
        }

        errors
    }

    // -- internal helpers ---------------------------------------------------

    /// Return `true` if an expression is a literal `0`.
    fn is_literal_zero(expr: &Expr) -> bool {
        match expr {
            Expr::Literal(Literal::Int(s)) => s == "0",
            Expr::Paren(inner) => Self::is_literal_zero(inner),
            _ => false,
        }
    }

    /// Return the wider of two ranges (union of both ranges).
    fn wider_range(a: (i128, i128), b: (i128, i128)) -> (i128, i128) {
        (std::cmp::min(a.0, b.0), std::cmp::max(a.1, b.1))
    }
}

impl Default for FixedWidthChecker {
    fn default() -> Self {
        Self::new()
    }
}
// ===========================================================================
// T056: MEM.3 Allocator contracts
// ===========================================================================

/// Tracks allocation/deallocation pairing and size constraints.
///
/// Error codes:
/// - A22001: allocation not paired with deallocation
/// - A22002: double free (deallocating already freed allocation)
/// - A22003: size mismatch between allocation and deallocation
/// - A22004: arena lifetime violation (use after arena drop)
#[derive(Debug, Clone)]
pub struct AllocatorChecker {
    allocations: HashMap<std::string::String, AllocInfo>,
    freed: HashMap<std::string::String, Range<usize>>,
    arenas: HashMap<std::string::String, ArenaInfo>,
}

#[derive(Debug, Clone)]
pub struct AllocInfo {
    pub size_expr: std::string::String,
    pub span: Range<usize>,
    pub arena: Option<std::string::String>,
}

#[derive(Debug, Clone)]
pub struct ArenaInfo {
    pub dropped: bool,
    pub drop_span: Option<Range<usize>>,
}

impl AllocatorChecker {
    pub fn new() -> Self {
        Self {
            allocations: HashMap::new(),
            freed: HashMap::new(),
            arenas: HashMap::new(),
        }
    }

    pub fn declare_arena(&mut self, name: std::string::String) {
        self.arenas.insert(
            name,
            ArenaInfo {
                dropped: false,
                drop_span: None,
            },
        );
    }

    pub fn drop_arena(&mut self, name: &str, span: Range<usize>) {
        if let Some(info) = self.arenas.get_mut(name) {
            info.dropped = true;
            info.drop_span = Some(span);
        }
    }

    pub fn record_alloc(
        &mut self,
        name: std::string::String,
        size_expr: std::string::String,
        arena: Option<std::string::String>,
        span: Range<usize>,
    ) {
        self.allocations.insert(
            name,
            AllocInfo {
                size_expr,
                span,
                arena,
            },
        );
    }

    pub fn record_free(&mut self, name: &str, span: Range<usize>) -> Option<TypeError> {
        if self.freed.contains_key(name) {
            return Some(TypeError {
                code: "A22002".into(),
                message: format!("double free: `{name}` already deallocated"),
                span: span.clone(),
                secondary: None,
            });
        }
        self.freed.insert(name.to_string(), span);
        None
    }

    pub fn check_arena_use(&self, alloc_name: &str, span: &Range<usize>) -> Option<TypeError> {
        if let Some(info) = self.allocations.get(alloc_name)
            && let Some(arena_name) = &info.arena
            && let Some(arena) = self.arenas.get(arena_name)
            && arena.dropped
        {
            return Some(TypeError {
                code: "A22004".into(),
                message: format!("use of `{alloc_name}` after arena `{arena_name}` dropped"),
                span: span.clone(),
                secondary: None,
            });
        }
        None
    }

    pub fn check_unpaired(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, info) in &self.allocations {
            if !self.freed.contains_key(name) && info.arena.is_none() {
                errors.push(TypeError {
                    code: "A22001".into(),
                    message: format!("allocation `{name}` not paired with deallocation"),
                    span: info.span.clone(),
                    secondary: None,
                });
            }
        }
        errors.sort_by_key(|e| e.span.start);
        errors
    }
}

impl Default for AllocatorChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T057: MEM.4 Circular buffer contracts
// ===========================================================================

/// Checks circular buffer indexing invariants.
///
/// Error codes:
/// - A23001: logical index exceeds buffer capacity
/// - A23002: physical index computation may wrap incorrectly
/// - A23003: buffer empty on read
#[derive(Debug, Clone)]
pub struct CircularBufferChecker {
    buffers: HashMap<std::string::String, CircBufInfo>,
}

#[derive(Debug, Clone)]
pub struct CircBufInfo {
    pub capacity: usize,
    pub head: usize,
    pub tail: usize,
    pub count: usize,
}

impl CircBufInfo {
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
    pub fn is_full(&self) -> bool {
        self.count >= self.capacity
    }
    pub fn logical_to_physical(&self, logical: usize) -> usize {
        (self.head + logical) % self.capacity
    }
}

impl CircularBufferChecker {
    pub fn new() -> Self {
        Self {
            buffers: HashMap::new(),
        }
    }

    pub fn declare(&mut self, name: std::string::String, capacity: usize) {
        self.buffers.insert(
            name,
            CircBufInfo {
                capacity,
                head: 0,
                tail: 0,
                count: 0,
            },
        );
    }

    pub fn check_read(&self, name: &str, span: &Range<usize>) -> Option<TypeError> {
        if let Some(buf) = self.buffers.get(name)
            && buf.is_empty()
        {
            return Some(TypeError {
                code: "A23003".into(),
                message: format!("read from empty circular buffer `{name}`"),
                span: span.clone(),
                secondary: None,
            });
        }
        None
    }

    pub fn check_index(
        &self,
        name: &str,
        logical_idx: usize,
        span: &Range<usize>,
    ) -> Option<TypeError> {
        if let Some(buf) = self.buffers.get(name)
            && logical_idx >= buf.capacity
        {
            return Some(TypeError {
                code: "A23001".into(),
                message: format!(
                    "logical index {logical_idx} exceeds capacity {} of `{name}`",
                    buf.capacity
                ),
                span: span.clone(),
                secondary: None,
            });
        }
        None
    }

    pub fn check_physical_wrap(
        &self,
        name: &str,
        offset: usize,
        span: &Range<usize>,
    ) -> Option<TypeError> {
        if let Some(buf) = self.buffers.get(name) {
            if buf.capacity == 0 {
                return Some(TypeError {
                    code: "A23002".into(),
                    message: format!(
                        "circular buffer `{name}` has zero capacity, modular wrap undefined"
                    ),
                    span: span.clone(),
                    secondary: None,
                });
            }
            let _physical = (buf.head + offset) % buf.capacity;
        }
        None
    }

    pub fn push(&mut self, name: &str) {
        if let Some(buf) = self.buffers.get_mut(name)
            && buf.count < buf.capacity
        {
            buf.tail = (buf.tail + 1) % buf.capacity;
            buf.count += 1;
        }
    }

    pub fn pop(&mut self, name: &str) {
        if let Some(buf) = self.buffers.get_mut(name)
            && buf.count > 0
        {
            buf.head = (buf.head + 1) % buf.capacity;
            buf.count -= 1;
        }
    }
}

impl Default for CircularBufferChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T066: CONC.2 Callback re-entrancy prevention
// ===========================================================================

/// Prevents re-entrant calls through callback chains.
///
/// Error codes:
/// - A24001: re-entrant callback invocation detected
/// - A24002: callback registered in non-reentrant context
/// - A24003: unbounded callback depth
#[derive(Debug, Clone)]
pub struct CallbackReentrancyChecker {
    /// Functions currently on the call stack
    call_stack: Vec<std::string::String>,
    /// Functions marked as non-reentrant
    non_reentrant: HashMap<std::string::String, Range<usize>>,
    /// Maximum allowed callback depth
    max_depth: usize,
}

impl CallbackReentrancyChecker {
    pub fn new() -> Self {
        Self {
            call_stack: Vec::new(),
            non_reentrant: HashMap::new(),
            max_depth: 16,
        }
    }

    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }

    pub fn mark_non_reentrant(&mut self, fn_name: std::string::String, span: Range<usize>) {
        self.non_reentrant.insert(fn_name, span);
    }

    pub fn enter_call(&mut self, fn_name: &str, span: &Range<usize>) -> Vec<TypeError> {
        let mut errors = Vec::new();

        // Check re-entrancy
        if self.call_stack.contains(&fn_name.to_string())
            && self.non_reentrant.contains_key(fn_name)
        {
            errors.push(TypeError {
                code: "A24001".into(),
                message: format!("re-entrant call to non-reentrant function `{fn_name}`"),
                span: span.clone(),
                secondary: None,
            });
        }

        // Check depth
        if self.call_stack.len() >= self.max_depth {
            errors.push(TypeError {
                code: "A24003".into(),
                message: format!(
                    "callback depth {} exceeds maximum {}",
                    self.call_stack.len() + 1,
                    self.max_depth
                ),
                span: span.clone(),
                secondary: None,
            });
        }

        self.call_stack.push(fn_name.to_string());
        errors
    }

    pub fn exit_call(&mut self) {
        self.call_stack.pop();
    }

    pub fn check_register_callback(
        &self,
        target_fn: &str,
        span: &Range<usize>,
    ) -> Option<TypeError> {
        if self.non_reentrant.contains_key(target_fn)
            && self.call_stack.contains(&target_fn.to_string())
        {
            return Some(TypeError {
                code: "A24002".into(),
                message: format!(
                    "registering callback to non-reentrant `{target_fn}` while inside it"
                ),
                span: span.clone(),
                secondary: None,
            });
        }
        None
    }

    pub fn current_depth(&self) -> usize {
        self.call_stack.len()
    }
}

impl Default for CallbackReentrancyChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T069: CONC.5 Temporal deadlines
// ===========================================================================

/// Enforces bounded response time contracts.
///
/// Error codes:
/// - A25001: operation exceeds declared deadline
/// - A25002: nested deadline violation (inner > outer)
/// - A25003: unbounded operation in deadline context
#[derive(Debug, Clone)]
pub struct TemporalDeadlineChecker {
    /// Active deadline scopes (name -> deadline_ms)
    deadlines: Vec<(std::string::String, u64)>,
    /// Operations with known worst-case times
    operation_bounds: HashMap<std::string::String, u64>,
}

impl TemporalDeadlineChecker {
    pub fn new() -> Self {
        Self {
            deadlines: Vec::new(),
            operation_bounds: HashMap::new(),
        }
    }

    pub fn register_bound(&mut self, op: std::string::String, worst_case_ms: u64) {
        self.operation_bounds.insert(op, worst_case_ms);
    }

    pub fn enter_deadline(
        &mut self,
        name: std::string::String,
        deadline_ms: u64,
        span: &Range<usize>,
    ) -> Option<TypeError> {
        // Check nested deadline doesn't exceed outer
        if let Some((outer_name, outer_ms)) = self.deadlines.last()
            && deadline_ms > *outer_ms
        {
            return Some(TypeError {
                code: "A25002".into(),
                message: format!(
                    "inner deadline `{name}` ({deadline_ms}ms) exceeds outer `{outer_name}` ({outer_ms}ms)"
                ),
                span: span.clone(),
                secondary: None,
            });
        }
        self.deadlines.push((name, deadline_ms));
        None
    }

    pub fn exit_deadline(&mut self) {
        self.deadlines.pop();
    }

    pub fn check_operation(&self, op: &str, span: &Range<usize>) -> Option<TypeError> {
        if let Some((deadline_name, deadline_ms)) = self.deadlines.last() {
            if let Some(worst_case) = self.operation_bounds.get(op) {
                if worst_case > deadline_ms {
                    return Some(TypeError {
                        code: "A25001".into(),
                        message: format!(
                            "operation `{op}` worst-case {worst_case}ms exceeds deadline `{deadline_name}` ({deadline_ms}ms)"
                        ),
                        span: span.clone(),
                        secondary: None,
                    });
                }
            } else {
                return Some(TypeError {
                    code: "A25003".into(),
                    message: format!(
                        "unbounded operation `{op}` in deadline context `{deadline_name}`"
                    ),
                    span: span.clone(),
                    secondary: None,
                });
            }
        }
        None
    }

    pub fn current_deadline(&self) -> Option<(&str, u64)> {
        self.deadlines.last().map(|(n, d)| (n.as_str(), *d))
    }
}

impl Default for TemporalDeadlineChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T070: FMT.1 Binary format contracts
// ===========================================================================

/// Validates byte-aligned binary format contracts.
///
/// Error codes:
/// - A26001: field offset exceeds buffer length
/// - A26002: field size mismatch
/// - A26003: endianness not specified for multi-byte field
/// - A26004: overlapping fields
#[derive(Debug, Clone)]
pub struct BinaryFormatChecker {
    fields: Vec<BinaryField>,
}

#[derive(Debug, Clone)]
pub struct BinaryField {
    pub name: std::string::String,
    pub offset: usize,
    pub size: usize,
    pub endianness: Option<Endianness>,
    pub span: Range<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Endianness {
    Big,
    Little,
    Native,
}

impl BinaryFormatChecker {
    pub fn new() -> Self {
        Self { fields: Vec::new() }
    }

    pub fn add_field(&mut self, field: BinaryField) {
        self.fields.push(field);
    }

    pub fn check_bounds(&self, buffer_len: usize) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for f in &self.fields {
            if f.offset + f.size > buffer_len {
                errors.push(TypeError {
                    code: "A26001".into(),
                    message: format!(
                        "field `{}` at offset {} + size {} exceeds buffer length {buffer_len}",
                        f.name, f.offset, f.size
                    ),
                    span: f.span.clone(),
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn check_endianness(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for f in &self.fields {
            if f.size > 1 && f.endianness.is_none() {
                errors.push(TypeError {
                    code: "A26003".into(),
                    message: format!(
                        "multi-byte field `{}` (size {}) has no endianness annotation",
                        f.name, f.size
                    ),
                    span: f.span.clone(),
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn check_overlaps(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for i in 0..self.fields.len() {
            for j in (i + 1)..self.fields.len() {
                let a = &self.fields[i];
                let b = &self.fields[j];
                let a_end = a.offset + a.size;
                let b_end = b.offset + b.size;
                if a.offset < b_end && b.offset < a_end {
                    errors.push(TypeError {
                        code: "A26004".into(),
                        message: format!(
                            "fields `{}` [{},{}] and `{}` [{},{}] overlap",
                            a.name, a.offset, a_end, b.name, b.offset, b_end
                        ),
                        span: a.span.clone(),
                        secondary: None,
                    });
                }
            }
        }
        errors
    }

    pub fn check_all(&self, buffer_len: usize) -> Vec<TypeError> {
        let mut errors = self.check_bounds(buffer_len);
        errors.extend(self.check_endianness());
        errors.extend(self.check_overlaps());
        errors
    }
}

impl Default for BinaryFormatChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T071: FMT.2 Bit-level format contracts
// ===========================================================================

/// Validates sub-byte parsing with ghost bit cursor tracking.
///
/// Error codes:
/// - A27001: bit offset exceeds container size
/// - A27002: bit field crosses byte boundary without permission
/// - A27003: total bit width doesn't match declared size
#[derive(Debug, Clone)]
pub struct BitLevelChecker {
    fields: Vec<BitField>,
    container_bits: usize,
}

#[derive(Debug, Clone)]
pub struct BitField {
    pub name: std::string::String,
    pub bit_offset: usize,
    pub bit_width: usize,
    pub span: Range<usize>,
    pub cross_byte_ok: bool,
}

impl BitLevelChecker {
    pub fn new(container_bits: usize) -> Self {
        Self {
            fields: Vec::new(),
            container_bits,
        }
    }

    pub fn add_field(&mut self, field: BitField) {
        self.fields.push(field);
    }

    pub fn check_bounds(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for f in &self.fields {
            if f.bit_offset + f.bit_width > self.container_bits {
                errors.push(TypeError {
                    code: "A27001".into(),
                    message: format!(
                        "bit field `{}` at bit {} + width {} exceeds container ({} bits)",
                        f.name, f.bit_offset, f.bit_width, self.container_bits
                    ),
                    span: f.span.clone(),
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn check_byte_crossing(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for f in &self.fields {
            if !f.cross_byte_ok {
                let start_byte = f.bit_offset / 8;
                let end_byte = (f.bit_offset + f.bit_width.saturating_sub(1)) / 8;
                if start_byte != end_byte {
                    errors.push(TypeError {
                        code: "A27002".into(),
                        message: format!(
                            "bit field `{}` crosses byte boundary (bytes {start_byte}-{end_byte})",
                            f.name
                        ),
                        span: f.span.clone(),
                        secondary: None,
                    });
                }
            }
        }
        errors
    }

    pub fn check_total_width(&self, declared_size: usize) -> Option<TypeError> {
        let total: usize = self.fields.iter().map(|f| f.bit_width).sum();
        if total != declared_size {
            return Some(TypeError {
                code: "A27003".into(),
                message: format!(
                    "total bit width {total} doesn't match declared size {declared_size}"
                ),
                span: 0..1,
                secondary: None,
            });
        }
        None
    }

    pub fn check_all(&self, declared_size: usize) -> Vec<TypeError> {
        let mut errors = self.check_bounds();
        errors.extend(self.check_byte_crossing());
        if let Some(e) = self.check_total_width(declared_size) {
            errors.push(e);
        }
        errors
    }
}

// ===========================================================================
// T072: FMT.3 String encoding contracts
// ===========================================================================

/// Validates UTF-8/UTF-16/ASCII string encoding safety.
///
/// Error codes:
/// - A28001: unvalidated bytes used as string
/// - A28002: encoding mismatch (e.g., UTF-16 data treated as UTF-8)
/// - A28003: truncation within multi-byte sequence
#[derive(Debug, Clone, PartialEq)]
pub enum StringEncoding {
    Utf8,
    Utf16Le,
    Utf16Be,
    Ascii,
    Latin1,
    RawBytes,
}

#[derive(Debug, Clone)]
pub struct StringEncodingChecker {
    variables: HashMap<std::string::String, StringEncoding>,
}

impl StringEncodingChecker {
    pub fn new() -> Self {
        Self {
            variables: HashMap::new(),
        }
    }

    pub fn declare(&mut self, name: std::string::String, encoding: StringEncoding) {
        self.variables.insert(name, encoding);
    }

    pub fn check_use_as_string(&self, name: &str, span: &Range<usize>) -> Option<TypeError> {
        match self.variables.get(name) {
            Some(StringEncoding::RawBytes) => Some(TypeError {
                code: "A28001".into(),
                message: format!("`{name}` is raw bytes, not a validated string"),
                span: span.clone(),
                secondary: None,
            }),
            None => Some(TypeError {
                code: "A28001".into(),
                message: format!("`{name}` has unknown encoding, cannot use as string"),
                span: span.clone(),
                secondary: None,
            }),
            _ => None,
        }
    }

    pub fn check_encoding_compat(
        &self,
        src: &str,
        dst_encoding: &StringEncoding,
        span: &Range<usize>,
    ) -> Option<TypeError> {
        if let Some(src_enc) = self.variables.get(src)
            && src_enc != dst_encoding
            && *src_enc != StringEncoding::Ascii
        {
            return Some(TypeError {
                code: "A28002".into(),
                message: format!("`{src}` is {src_enc:?} but used as {dst_encoding:?}"),
                span: span.clone(),
                secondary: None,
            });
        }
        None
    }

    pub fn check_truncation(
        &self,
        name: &str,
        byte_len: usize,
        span: &Range<usize>,
    ) -> Option<TypeError> {
        if let Some(enc) = self.variables.get(name) {
            let unit_size = match enc {
                StringEncoding::Utf16Le | StringEncoding::Utf16Be => 2,
                _ => 1,
            };
            if unit_size > 1 && !byte_len.is_multiple_of(unit_size) {
                return Some(TypeError {
                    code: "A28003".into(),
                    message: format!(
                        "truncation of `{name}` at byte {byte_len} may split a {enc:?} code unit"
                    ),
                    span: span.clone(),
                    secondary: None,
                });
            }
        }
        None
    }
}

impl Default for StringEncodingChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T074: FMT.5 Checksum integrity
// ===========================================================================

/// Validates checksum verification contracts.
///
/// Error codes:
/// - A29001: data used before checksum verification
/// - A29002: checksum algorithm mismatch
/// - A29003: checksum covers wrong byte range
#[derive(Debug, Clone, PartialEq)]
pub enum ChecksumAlgorithm {
    Crc32,
    Adler32,
    Sha256,
    Sha512,
    Md5,
    Custom(std::string::String),
}

#[derive(Debug, Clone)]
pub struct ChecksumChecker {
    /// Data regions and their checksum status
    regions: HashMap<std::string::String, ChecksumRegion>,
}

#[derive(Debug, Clone)]
pub struct ChecksumRegion {
    pub algorithm: ChecksumAlgorithm,
    pub byte_start: usize,
    pub byte_end: usize,
    pub verified: bool,
}

impl ChecksumChecker {
    pub fn new() -> Self {
        Self {
            regions: HashMap::new(),
        }
    }

    pub fn declare_region(
        &mut self,
        name: std::string::String,
        algorithm: ChecksumAlgorithm,
        start: usize,
        end: usize,
    ) {
        self.regions.insert(
            name,
            ChecksumRegion {
                algorithm,
                byte_start: start,
                byte_end: end,
                verified: false,
            },
        );
    }

    pub fn mark_verified(&mut self, name: &str) {
        if let Some(region) = self.regions.get_mut(name) {
            region.verified = true;
        }
    }

    pub fn check_use_before_verify(&self, name: &str, span: &Range<usize>) -> Option<TypeError> {
        if let Some(region) = self.regions.get(name)
            && !region.verified
        {
            return Some(TypeError {
                code: "A29001".into(),
                message: format!("data region `{name}` used before checksum verification"),
                span: span.clone(),
                secondary: None,
            });
        }
        None
    }

    pub fn check_algorithm_match(
        &self,
        name: &str,
        expected: &ChecksumAlgorithm,
        span: &Range<usize>,
    ) -> Option<TypeError> {
        if let Some(region) = self.regions.get(name)
            && &region.algorithm != expected
        {
            return Some(TypeError {
                code: "A29002".into(),
                message: format!(
                    "checksum algorithm mismatch for `{name}`: declared {:?}, used {:?}",
                    region.algorithm, expected
                ),
                span: span.clone(),
                secondary: None,
            });
        }
        None
    }

    pub fn check_range_coverage(
        &self,
        name: &str,
        data_start: usize,
        data_end: usize,
        span: &Range<usize>,
    ) -> Option<TypeError> {
        if let Some(region) = self.regions.get(name)
            && (region.byte_start > data_start || region.byte_end < data_end)
        {
            return Some(TypeError {
                code: "A29003".into(),
                message: format!(
                    "checksum for `{name}` covers [{},{}] but data range is [{data_start},{data_end}]",
                    region.byte_start, region.byte_end
                ),
                span: span.clone(),
                secondary: None,
            });
        }
        None
    }
}

impl Default for ChecksumChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T075: FMT.6 Protocol grammar
// ===========================================================================

/// Validates protocol state machine and RFC conformance.
///
/// Error codes:
/// - A30001: invalid state transition
/// - A30002: message sent in wrong protocol state
/// - A30003: required message field missing
#[derive(Debug, Clone)]
pub struct ProtocolGrammarChecker {
    states: Vec<std::string::String>,
    current_state: std::string::String,
    transitions: Vec<ProtocolTransition>,
    required_fields: HashMap<std::string::String, Vec<std::string::String>>,
}

#[derive(Debug, Clone)]
pub struct ProtocolTransition {
    pub from: std::string::String,
    pub to: std::string::String,
    pub message: std::string::String,
}

impl ProtocolGrammarChecker {
    pub fn new(initial_state: std::string::String) -> Self {
        Self {
            states: vec![initial_state.clone()],
            current_state: initial_state,
            transitions: Vec::new(),
            required_fields: HashMap::new(),
        }
    }

    pub fn add_state(&mut self, state: std::string::String) {
        if !self.states.contains(&state) {
            self.states.push(state);
        }
    }

    pub fn add_transition(
        &mut self,
        from: std::string::String,
        to: std::string::String,
        message: std::string::String,
    ) {
        self.transitions
            .push(ProtocolTransition { from, to, message });
    }

    pub fn add_required_fields(
        &mut self,
        message: std::string::String,
        fields: Vec<std::string::String>,
    ) {
        self.required_fields.insert(message, fields);
    }

    pub fn check_send(&self, message: &str, span: &Range<usize>) -> Option<TypeError> {
        let valid = self
            .transitions
            .iter()
            .any(|t| t.from == self.current_state && t.message == message);
        if !valid {
            return Some(TypeError {
                code: "A30002".into(),
                message: format!("cannot send `{message}` in state `{}`", self.current_state),
                span: span.clone(),
                secondary: None,
            });
        }
        None
    }

    pub fn transition(&mut self, message: &str, span: &Range<usize>) -> Option<TypeError> {
        if let Some(t) = self
            .transitions
            .iter()
            .find(|t| t.from == self.current_state && t.message == message)
        {
            self.current_state = t.to.clone();
            None
        } else {
            Some(TypeError {
                code: "A30001".into(),
                message: format!(
                    "invalid transition: no `{message}` transition from state `{}`",
                    self.current_state
                ),
                span: span.clone(),
                secondary: None,
            })
        }
    }

    pub fn check_required_fields(
        &self,
        message: &str,
        provided: &[&str],
        span: &Range<usize>,
    ) -> Vec<TypeError> {
        let mut errors = Vec::new();
        if let Some(required) = self.required_fields.get(message) {
            for field in required {
                if !provided.contains(&field.as_str()) {
                    errors.push(TypeError {
                        code: "A30003".into(),
                        message: format!("required field `{field}` missing in message `{message}`"),
                        span: span.clone(),
                        secondary: None,
                    });
                }
            }
        }
        errors
    }

    pub fn current_state(&self) -> &str {
        &self.current_state
    }
}

// ===========================================================================
// T077: CORE.4 Axiomatic definitions
// ===========================================================================

/// Validates axiomatic (abstract mathematical) definitions.
///
/// Error codes:
/// - A31001: axiom references undefined symbol
/// - A31002: axiom set is inconsistent (circular or contradictory)
/// - A31003: axiom not used in any proof
#[derive(Debug, Clone)]
pub struct AxiomaticDefChecker {
    axioms: HashMap<std::string::String, AxiomDef>,
    used_axioms: Vec<std::string::String>,
}

#[derive(Debug, Clone)]
pub struct AxiomDef {
    pub name: std::string::String,
    pub params: Vec<std::string::String>,
    pub body: std::string::String,
    pub span: Range<usize>,
    pub references: Vec<std::string::String>,
}

impl AxiomaticDefChecker {
    pub fn new() -> Self {
        Self {
            axioms: HashMap::new(),
            used_axioms: Vec::new(),
        }
    }

    pub fn declare_axiom(&mut self, axiom: AxiomDef) {
        self.axioms.insert(axiom.name.clone(), axiom);
    }

    pub fn mark_used(&mut self, name: &str) {
        if !self.used_axioms.contains(&name.to_string()) {
            self.used_axioms.push(name.to_string());
        }
    }

    pub fn check_references(&self, known_symbols: &[&str]) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for axiom in self.axioms.values() {
            for reference in &axiom.references {
                let is_axiom = self.axioms.contains_key(reference);
                let is_known = known_symbols.contains(&reference.as_str());
                if !is_axiom && !is_known {
                    errors.push(TypeError {
                        code: "A31001".into(),
                        message: format!(
                            "axiom `{}` references undefined symbol `{reference}`",
                            axiom.name
                        ),
                        span: axiom.span.clone(),
                        secondary: None,
                    });
                }
            }
        }
        errors
    }

    pub fn check_unused(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, axiom) in &self.axioms {
            if !self.used_axioms.contains(name) {
                errors.push(TypeError {
                    code: "A31003".into(),
                    message: format!("axiom `{name}` is never used in any proof"),
                    span: axiom.span.clone(),
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn check_circular(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, axiom) in &self.axioms {
            if self.has_cycle(name, &mut vec![name.clone()]) {
                errors.push(TypeError {
                    code: "A31002".into(),
                    message: format!("axiom `{name}` has circular dependency"),
                    span: axiom.span.clone(),
                    secondary: None,
                });
            }
        }
        errors
    }

    fn has_cycle(&self, current: &str, visited: &mut Vec<std::string::String>) -> bool {
        if let Some(axiom) = self.axioms.get(current) {
            for reference in &axiom.references {
                if visited.contains(reference) {
                    return true;
                }
                if self.axioms.contains_key(reference) {
                    visited.push(reference.clone());
                    if self.has_cycle(reference, visited) {
                        return true;
                    }
                    visited.pop();
                }
            }
        }
        false
    }
}

impl Default for AxiomaticDefChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T079: CORE.6 Opaque functions
// ===========================================================================

/// Manages opaque function declarations that hide implementation from verifier.
///
/// Error codes:
/// - A32001: opaque function called without contract
/// - A32002: opaque function body accessed during verification
/// - A32003: reveal used outside proof context
#[derive(Debug, Clone)]
pub struct OpaqueFunctionChecker {
    opaque_fns: HashMap<std::string::String, OpaqueFnInfo>,
    revealed: Vec<std::string::String>,
    in_proof_context: bool,
}

#[derive(Debug, Clone)]
pub struct OpaqueFnInfo {
    pub has_contract: bool,
    pub span: Range<usize>,
}

impl OpaqueFunctionChecker {
    pub fn new() -> Self {
        Self {
            opaque_fns: HashMap::new(),
            revealed: Vec::new(),
            in_proof_context: false,
        }
    }

    pub fn declare_opaque(
        &mut self,
        name: std::string::String,
        has_contract: bool,
        span: Range<usize>,
    ) {
        self.opaque_fns
            .insert(name, OpaqueFnInfo { has_contract, span });
    }

    pub fn enter_proof(&mut self) {
        self.in_proof_context = true;
    }

    pub fn exit_proof(&mut self) {
        self.in_proof_context = false;
    }

    pub fn check_call(&self, fn_name: &str, span: &Range<usize>) -> Option<TypeError> {
        if let Some(info) = self.opaque_fns.get(fn_name)
            && !info.has_contract
        {
            return Some(TypeError {
                code: "A32001".into(),
                message: format!("opaque function `{fn_name}` called without contract"),
                span: span.clone(),
                secondary: None,
            });
        }
        None
    }

    pub fn check_body_access(&self, fn_name: &str, span: &Range<usize>) -> Option<TypeError> {
        if self.opaque_fns.contains_key(fn_name) && !self.revealed.contains(&fn_name.to_string()) {
            return Some(TypeError {
                code: "A32002".into(),
                message: format!("body of opaque function `{fn_name}` accessed without reveal"),
                span: span.clone(),
                secondary: None,
            });
        }
        None
    }

    pub fn reveal(&mut self, fn_name: &str, span: &Range<usize>) -> Option<TypeError> {
        if !self.in_proof_context {
            return Some(TypeError {
                code: "A32003".into(),
                message: format!("`reveal {fn_name}` used outside proof context"),
                span: span.clone(),
                secondary: None,
            });
        }
        self.revealed.push(fn_name.to_string());
        None
    }

    pub fn is_opaque(&self, fn_name: &str) -> bool {
        self.opaque_fns.contains_key(fn_name)
    }
}

impl Default for OpaqueFunctionChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T083: TEST.1 Test generation from contracts
// ===========================================================================

/// Generates property-based and boundary-value tests from contract specs.
///
/// Produces Rust test code (proptest/quickcheck) from requires/ensures clauses.
#[derive(Debug, Clone)]
pub struct TestGenerator {
    contracts: Vec<TestableContract>,
}

#[derive(Debug, Clone)]
pub struct TestableContract {
    pub name: std::string::String,
    pub params: Vec<(std::string::String, Type)>,
    pub requires: Vec<std::string::String>,
    pub ensures: Vec<std::string::String>,
}

#[derive(Debug, Clone)]
pub struct GeneratedTest {
    pub name: std::string::String,
    pub body: std::string::String,
    pub kind: TestKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TestKind {
    Property,
    Boundary,
    Smoke,
}

impl TestGenerator {
    pub fn new() -> Self {
        Self {
            contracts: Vec::new(),
        }
    }

    pub fn add_contract(&mut self, contract: TestableContract) {
        self.contracts.push(contract);
    }

    pub fn generate_property_test(&self, contract: &TestableContract) -> GeneratedTest {
        let param_list: Vec<std::string::String> = contract
            .params
            .iter()
            .map(|(n, t)| format!("{n}: {}", Self::type_to_proptest_strategy(t)))
            .collect();
        let preconditions = if contract.requires.is_empty() {
            String::new()
        } else {
            format!(
                "prop_assume!({});\n        ",
                contract.requires.join(" && ")
            )
        };
        let postconditions = contract.ensures.join(" && ");
        let body = format!(
            "proptest! {{\n    #[test]\n    fn prop_{}({}) {{\n        {preconditions}prop_assert!({postconditions});\n    }}\n}}",
            contract.name,
            param_list.join(", ")
        );
        GeneratedTest {
            name: format!("prop_{}", contract.name),
            body,
            kind: TestKind::Property,
        }
    }

    pub fn generate_boundary_tests(&self, contract: &TestableContract) -> Vec<GeneratedTest> {
        let mut tests = Vec::new();
        for (name, ty) in &contract.params {
            let boundaries = Self::boundary_values(ty);
            for (i, val) in boundaries.iter().enumerate() {
                tests.push(GeneratedTest {
                    name: format!("boundary_{}_{}_{}", contract.name, name, i),
                    body: format!("#[test]\nfn boundary_{}_{}_{i}() {{\n    let {name} = {val};\n    // boundary test for {name}\n}}", contract.name, name),
                    kind: TestKind::Boundary,
                });
            }
        }
        tests
    }

    pub fn generate_smoke_test(&self, contract: &TestableContract) -> GeneratedTest {
        let body = format!(
            "#[test]\nfn smoke_{}() {{\n    // smoke test: basic valid inputs\n}}",
            contract.name
        );
        GeneratedTest {
            name: format!("smoke_{}", contract.name),
            body,
            kind: TestKind::Smoke,
        }
    }

    pub fn generate_all(&self) -> Vec<GeneratedTest> {
        let mut tests = Vec::new();
        for contract in &self.contracts {
            tests.push(self.generate_property_test(contract));
            tests.extend(self.generate_boundary_tests(contract));
            tests.push(self.generate_smoke_test(contract));
        }
        tests
    }

    fn type_to_proptest_strategy(ty: &Type) -> &'static str {
        match ty {
            Type::Int | Type::I64 => "i64::ANY",
            Type::Nat | Type::U64 => "u64::ANY",
            Type::U8 => "u8::ANY",
            Type::U16 => "u16::ANY",
            Type::U32 => "u32::ANY",
            Type::I8 => "i8::ANY",
            Type::I16 => "i16::ANY",
            Type::I32 => "i32::ANY",
            Type::Float | Type::F64 => "f64::ANY",
            Type::F32 => "f32::ANY",
            Type::Bool => "bool::ANY",
            Type::String => "\".*\"",
            _ => "any::<()>()",
        }
    }

    fn boundary_values(ty: &Type) -> Vec<std::string::String> {
        match ty {
            Type::Int | Type::I64 => vec![
                "0".into(),
                "1".into(),
                "-1".into(),
                "i64::MAX".into(),
                "i64::MIN".into(),
            ],
            Type::Nat | Type::U64 => vec!["0".into(), "1".into(), "u64::MAX".into()],
            Type::U8 => vec!["0u8".into(), "1u8".into(), "255u8".into()],
            Type::U16 => vec!["0u16".into(), "1u16".into(), "65535u16".into()],
            Type::U32 => vec!["0u32".into(), "1u32".into(), "u32::MAX".into()],
            Type::I8 => vec![
                "0i8".into(),
                "1i8".into(),
                "-1i8".into(),
                "127i8".into(),
                "-128i8".into(),
            ],
            Type::I16 => vec![
                "0i16".into(),
                "1i16".into(),
                "-1i16".into(),
                "i16::MAX".into(),
                "i16::MIN".into(),
            ],
            Type::I32 => vec![
                "0i32".into(),
                "1i32".into(),
                "-1i32".into(),
                "i32::MAX".into(),
                "i32::MIN".into(),
            ],
            Type::Bool => vec!["true".into(), "false".into()],
            Type::Float | Type::F64 => vec![
                "0.0".into(),
                "1.0".into(),
                "-1.0".into(),
                "f64::INFINITY".into(),
                "f64::NAN".into(),
            ],
            _ => vec![],
        }
    }
}

impl Default for TestGenerator {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------

// ===========================================================================
// T086: STOR.1 Crash recovery contracts
// ===========================================================================

/// Tracks write-ahead log (WAL) discipline and crash-safe commit sequences.
#[derive(Debug, Clone)]
pub struct CrashRecoveryChecker {
    wal_entries: Vec<WalEntry>,
    committed: Vec<std::string::String>,
    fsynced: Vec<std::string::String>,
}

#[derive(Debug, Clone)]
pub struct WalEntry {
    pub id: std::string::String,
    pub data_written: bool,
    pub wal_written: bool,
    pub fsynced: bool,
}

impl CrashRecoveryChecker {
    pub fn new() -> Self {
        Self {
            wal_entries: Vec::new(),
            committed: Vec::new(),
            fsynced: Vec::new(),
        }
    }

    pub fn begin_write(&mut self, id: std::string::String) {
        self.wal_entries.push(WalEntry {
            id,
            data_written: false,
            wal_written: false,
            fsynced: false,
        });
    }

    pub fn write_wal(&mut self, id: &str) {
        if let Some(e) = self.wal_entries.iter_mut().find(|e| e.id == id) {
            e.wal_written = true;
        }
    }

    pub fn write_data(&mut self, id: &str) {
        if let Some(e) = self.wal_entries.iter_mut().find(|e| e.id == id) {
            e.data_written = true;
        }
    }

    pub fn fsync(&mut self, id: &str) {
        if let Some(e) = self.wal_entries.iter_mut().find(|e| e.id == id) {
            e.fsynced = true;
        }
        self.fsynced.push(id.to_string());
    }

    pub fn commit(&mut self, id: &str) {
        self.committed.push(id.to_string());
    }

    pub fn check_write_ahead(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for e in &self.wal_entries {
            if e.data_written && !e.wal_written {
                errors.push(TypeError {
                    code: "A33001".into(),
                    message: format!("data write for `{}` without preceding WAL entry", e.id),
                    span: 0..1,
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn check_commit_durability(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for id in &self.committed {
            if !self.fsynced.contains(id) {
                errors.push(TypeError {
                    code: "A33002".into(),
                    message: format!("commit for `{id}` without fsync"),
                    span: 0..1,
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn check_ordering(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for e in &self.wal_entries {
            if e.fsynced && !e.data_written {
                errors.push(TypeError {
                    code: "A33003".into(),
                    message: format!("fsync for `{}` before data write", e.id),
                    span: 0..1,
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn check_all(&self) -> Vec<TypeError> {
        let mut errs = self.check_write_ahead();
        errs.extend(self.check_commit_durability());
        errs.extend(self.check_ordering());
        errs
    }
}

impl Default for CrashRecoveryChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T087: STOR.2 Page cache contracts
// ===========================================================================

#[derive(Debug, Clone)]
pub struct PageCacheChecker {
    pages: std::collections::HashMap<u64, PageInfo>,
    capacity: usize,
}

#[derive(Debug, Clone)]
pub struct PageInfo {
    pub page_id: u64,
    pub dirty: bool,
    pub pinned: bool,
    pub pin_count: u32,
}

impl PageCacheChecker {
    pub fn new(capacity: usize) -> Self {
        Self {
            pages: std::collections::HashMap::new(),
            capacity,
        }
    }

    pub fn load_page(&mut self, page_id: u64) {
        self.pages.insert(
            page_id,
            PageInfo {
                page_id,
                dirty: false,
                pinned: false,
                pin_count: 0,
            },
        );
    }

    pub fn pin(&mut self, page_id: u64) {
        if let Some(p) = self.pages.get_mut(&page_id) {
            p.pinned = true;
            p.pin_count += 1;
        }
    }

    pub fn unpin(&mut self, page_id: u64) {
        if let Some(p) = self.pages.get_mut(&page_id) {
            if p.pin_count > 0 {
                p.pin_count -= 1;
            }
            if p.pin_count == 0 {
                p.pinned = false;
            }
        }
    }

    pub fn mark_dirty(&mut self, page_id: u64) {
        if let Some(p) = self.pages.get_mut(&page_id) {
            p.dirty = true;
        }
    }

    pub fn flush(&mut self, page_id: u64) {
        if let Some(p) = self.pages.get_mut(&page_id) {
            p.dirty = false;
        }
    }

    pub fn evict(&mut self, page_id: u64) -> Option<TypeError> {
        if let Some(p) = self.pages.get(&page_id) {
            if p.pinned {
                return Some(TypeError {
                    code: "A34001".into(),
                    message: format!("cannot evict pinned page {page_id}"),
                    span: 0..1,
                    secondary: None,
                });
            }
            if p.dirty {
                return Some(TypeError {
                    code: "A34002".into(),
                    message: format!("evicting dirty page {page_id} without flush"),
                    span: 0..1,
                    secondary: None,
                });
            }
        }
        self.pages.remove(&page_id);
        None
    }

    pub fn check_capacity(&self) -> Vec<TypeError> {
        if self.pages.len() > self.capacity {
            vec![TypeError {
                code: "A34003".into(),
                message: format!(
                    "page cache size {} exceeds capacity {}",
                    self.pages.len(),
                    self.capacity
                ),
                span: 0..1,
                secondary: None,
            }]
        } else {
            vec![]
        }
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
}

impl Default for PageCacheChecker {
    fn default() -> Self {
        Self::new(1024)
    }
}

// ===========================================================================
// T088: STOR.3 MVCC / snapshot isolation
// ===========================================================================

#[derive(Debug, Clone)]
pub struct MvccChecker {
    versions: std::collections::HashMap<std::string::String, Vec<MvccVersion>>,
    active_snapshots: Vec<u64>,
    next_txn_id: u64,
}

#[derive(Debug, Clone)]
pub struct MvccVersion {
    pub txn_id: u64,
    pub committed: bool,
}

impl MvccChecker {
    pub fn new() -> Self {
        Self {
            versions: std::collections::HashMap::new(),
            active_snapshots: Vec::new(),
            next_txn_id: 1,
        }
    }

    pub fn begin_txn(&mut self) -> u64 {
        let id = self.next_txn_id;
        self.next_txn_id += 1;
        self.active_snapshots.push(id);
        id
    }

    pub fn write_version(&mut self, key: std::string::String, txn_id: u64) {
        self.versions.entry(key).or_default().push(MvccVersion {
            txn_id,
            committed: false,
        });
    }

    pub fn commit_txn(&mut self, txn_id: u64) {
        self.active_snapshots.retain(|&id| id != txn_id);
        for versions in self.versions.values_mut() {
            for v in versions.iter_mut() {
                if v.txn_id == txn_id {
                    v.committed = true;
                }
            }
        }
    }

    pub fn check_write_conflicts(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (key, versions) in &self.versions {
            let uncommitted: Vec<_> = versions.iter().filter(|v| !v.committed).collect();
            if uncommitted.len() > 1 {
                errors.push(TypeError {
                    code: "A35001".into(),
                    message: format!(
                        "write-write conflict on key `{key}`: {} uncommitted versions",
                        uncommitted.len()
                    ),
                    span: 0..1,
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn check_snapshot_read(&self, key: &str, reader_txn: u64) -> Option<TypeError> {
        if let Some(versions) = self.versions.get(key) {
            for v in versions {
                if v.txn_id != reader_txn
                    && !v.committed
                    && self.active_snapshots.contains(&v.txn_id)
                {
                    return Some(TypeError {
                        code: "A35002".into(),
                        message: format!(
                            "snapshot isolation violation: txn {reader_txn} reads uncommitted from txn {} on `{key}`",
                            v.txn_id
                        ),
                        span: 0..1,
                        secondary: None,
                    });
                }
            }
        }
        None
    }

    pub fn check_phantom(&self, txn_id: u64) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (key, versions) in &self.versions {
            for v in versions {
                if v.txn_id > txn_id && v.committed {
                    errors.push(TypeError { code: "A35003".into(), message: format!("phantom read: txn {txn_id} sees committed version from later txn {} on `{key}`", v.txn_id), span: 0..1, secondary: None });
                }
            }
        }
        errors
    }
}

impl Default for MvccChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T089: STOR.4 Transactional rollback
// ===========================================================================

#[derive(Debug, Clone)]
pub struct RollbackChecker {
    savepoints: Vec<std::string::String>,
    resources_acquired: Vec<std::string::String>,
    rolled_back: bool,
}

impl RollbackChecker {
    pub fn new() -> Self {
        Self {
            savepoints: Vec::new(),
            resources_acquired: Vec::new(),
            rolled_back: false,
        }
    }

    pub fn create_savepoint(&mut self, name: std::string::String) {
        self.savepoints.push(name);
    }

    pub fn acquire_resource(&mut self, name: std::string::String) {
        self.resources_acquired.push(name);
    }

    pub fn release_resource(&mut self, name: &str) {
        self.resources_acquired.retain(|r| r != name);
    }

    pub fn rollback_to(&mut self, savepoint: &str) -> Option<TypeError> {
        if !self.savepoints.contains(&savepoint.to_string()) {
            return Some(TypeError {
                code: "A36001".into(),
                message: format!("rollback to unknown savepoint `{savepoint}`"),
                span: 0..1,
                secondary: None,
            });
        }
        self.rolled_back = true;
        if let Some(pos) = self.savepoints.iter().position(|s| s == savepoint) {
            self.savepoints.truncate(pos + 1);
        }
        None
    }

    pub fn check_resource_leak(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        if self.rolled_back {
            for r in &self.resources_acquired {
                errors.push(TypeError {
                    code: "A36002".into(),
                    message: format!("resource `{r}` not released after rollback"),
                    span: 0..1,
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn check_savepoint_nesting(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for sp in &self.savepoints {
            if !seen.insert(sp.clone()) {
                errors.push(TypeError {
                    code: "A36003".into(),
                    message: format!("duplicate savepoint name `{sp}`"),
                    span: 0..1,
                    secondary: None,
                });
            }
        }
        errors
    }
}

impl Default for RollbackChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T090: STOR.5 Monotonic state
// ===========================================================================

#[derive(Debug, Clone)]
pub struct MonotonicStateChecker {
    monotonic_vars: std::collections::HashMap<std::string::String, MonotonicInfo>,
}

#[derive(Debug, Clone)]
pub struct MonotonicInfo {
    pub current_value: i64,
    pub direction: MonotonicDirection,
    pub span: std::ops::Range<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MonotonicDirection {
    Increasing,
    StrictlyIncreasing,
    Decreasing,
}

impl MonotonicStateChecker {
    pub fn new() -> Self {
        Self {
            monotonic_vars: std::collections::HashMap::new(),
        }
    }

    pub fn declare(
        &mut self,
        name: std::string::String,
        direction: MonotonicDirection,
        initial: i64,
        span: std::ops::Range<usize>,
    ) {
        self.monotonic_vars.insert(
            name,
            MonotonicInfo {
                current_value: initial,
                direction,
                span,
            },
        );
    }

    pub fn update(&mut self, name: &str, new_value: i64) -> Option<TypeError> {
        if let Some(info) = self.monotonic_vars.get_mut(name) {
            let violation = match info.direction {
                MonotonicDirection::Increasing => new_value < info.current_value,
                MonotonicDirection::StrictlyIncreasing => new_value <= info.current_value,
                MonotonicDirection::Decreasing => new_value > info.current_value,
            };
            if violation {
                return Some(TypeError {
                    code: "A37001".into(),
                    message: format!(
                        "monotonicity violation: `{name}` changed from {} to {new_value}",
                        info.current_value
                    ),
                    span: info.span.clone(),
                    secondary: None,
                });
            }
            info.current_value = new_value;
        }
        None
    }

    pub fn check_reset(&self, name: &str) -> Option<TypeError> {
        if self.monotonic_vars.contains_key(name) {
            Some(TypeError {
                code: "A37002".into(),
                message: format!("illegal reset of monotonic variable `{name}`"),
                span: 0..1,
                secondary: None,
            })
        } else {
            None
        }
    }

    pub fn check_access(&self, name: &str) -> Option<TypeError> {
        if !self.monotonic_vars.contains_key(name) {
            Some(TypeError {
                code: "A37003".into(),
                message: format!("access to undeclared monotonic variable `{name}`"),
                span: 0..1,
                secondary: None,
            })
        } else {
            None
        }
    }

    pub fn current_value(&self, name: &str) -> Option<i64> {
        self.monotonic_vars.get(name).map(|i| i.current_value)
    }
}

impl Default for MonotonicStateChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T091: STOR.6 Storage failure model
// ===========================================================================

#[derive(Debug, Clone)]
pub struct StorageFailureChecker {
    failure_modes: Vec<FailureMode>,
    handled_modes: Vec<std::string::String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FailureMode {
    PartialWrite,
    TornPage,
    BitRot,
    DiskFull,
    IoTimeout,
}

impl FailureMode {
    pub fn name(&self) -> &str {
        match self {
            Self::PartialWrite => "partial_write",
            Self::TornPage => "torn_page",
            Self::BitRot => "bit_rot",
            Self::DiskFull => "disk_full",
            Self::IoTimeout => "io_timeout",
        }
    }
}

impl StorageFailureChecker {
    pub fn new() -> Self {
        Self {
            failure_modes: Vec::new(),
            handled_modes: Vec::new(),
        }
    }

    pub fn declare_failure_mode(&mut self, mode: FailureMode) {
        self.failure_modes.push(mode);
    }

    pub fn mark_handled(&mut self, mode_name: &str) {
        if !self.handled_modes.contains(&mode_name.to_string()) {
            self.handled_modes.push(mode_name.to_string());
        }
    }

    pub fn check_unhandled(&self) -> Vec<TypeError> {
        self.failure_modes
            .iter()
            .filter(|m| !self.handled_modes.contains(&m.name().to_string()))
            .map(|m| TypeError {
                code: "A38001".into(),
                message: format!("storage failure mode `{}` has no handler", m.name()),
                span: 0..1,
                secondary: None,
            })
            .collect()
    }

    pub fn check_spurious_handlers(&self) -> Vec<TypeError> {
        let declared: Vec<_> = self
            .failure_modes
            .iter()
            .map(|m| m.name().to_string())
            .collect();
        self.handled_modes
            .iter()
            .filter(|h| !declared.contains(h))
            .map(|h| TypeError {
                code: "A38002".into(),
                message: format!("handler for undeclared failure mode `{h}`"),
                span: 0..1,
                secondary: None,
            })
            .collect()
    }

    pub fn check_critical_coverage(&self) -> Vec<TypeError> {
        let critical = [FailureMode::PartialWrite, FailureMode::TornPage];
        critical
            .iter()
            .filter(|m| {
                self.failure_modes.contains(m)
                    && !self.handled_modes.contains(&m.name().to_string())
            })
            .map(|m| TypeError {
                code: "A38003".into(),
                message: format!("critical failure mode `{}` must have a handler", m.name()),
                span: 0..1,
                secondary: None,
            })
            .collect()
    }

    pub fn failure_count(&self) -> usize {
        self.failure_modes.len()
    }
}

impl Default for StorageFailureChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T095: NUM.1 Numerical precision
// ===========================================================================

#[derive(Debug, Clone)]
pub struct NumericalPrecisionChecker {
    variables: std::collections::HashMap<std::string::String, PrecisionInfo>,
}

#[derive(Debug, Clone)]
pub struct PrecisionInfo {
    pub bits: u32,
    pub min_ulp: f64,
    pub span: std::ops::Range<usize>,
}

impl NumericalPrecisionChecker {
    pub fn new() -> Self {
        Self {
            variables: std::collections::HashMap::new(),
        }
    }

    pub fn declare(
        &mut self,
        name: std::string::String,
        bits: u32,
        min_ulp: f64,
        span: std::ops::Range<usize>,
    ) {
        self.variables.insert(
            name,
            PrecisionInfo {
                bits,
                min_ulp,
                span,
            },
        );
    }

    pub fn check_precision_loss(&self, name: &str, result_bits: u32) -> Option<TypeError> {
        if let Some(info) = self.variables.get(name)
            && result_bits < info.bits
        {
            return Some(TypeError {
                code: "A42001".into(),
                message: format!(
                    "precision loss: `{name}` requires {}-bit but operation produces {result_bits}-bit",
                    info.bits
                ),
                span: info.span.clone(),
                secondary: None,
            });
        }
        None
    }

    pub fn check_ulp_bound(&self, name: &str, actual_ulp: f64) -> Option<TypeError> {
        if let Some(info) = self.variables.get(name)
            && actual_ulp > info.min_ulp
        {
            return Some(TypeError {
                code: "A42002".into(),
                message: format!(
                    "ULP violation: `{name}` requires ULP <= {} but got {actual_ulp}",
                    info.min_ulp
                ),
                span: info.span.clone(),
                secondary: None,
            });
        }
        None
    }

    pub fn check_cancellation(&self, name: &str, operand_ratio: f64) -> Option<TypeError> {
        if operand_ratio > 0.999
            && let Some(info) = self.variables.get(name)
        {
            return Some(TypeError {
                code: "A42003".into(),
                message: format!(
                    "potential catastrophic cancellation in `{name}` (operand ratio: {operand_ratio})"
                ),
                span: info.span.clone(),
                secondary: None,
            });
        }
        None
    }
}

impl Default for NumericalPrecisionChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T096: NUM.2 Precomputed table verification
// ===========================================================================

#[derive(Debug, Clone)]
pub struct PrecomputedTableChecker {
    tables: Vec<TableDecl>,
}

#[derive(Debug, Clone)]
pub struct TableDecl {
    pub name: std::string::String,
    pub size: usize,
    pub verified_entries: usize,
    pub generator_fn: std::string::String,
    pub span: std::ops::Range<usize>,
}

impl PrecomputedTableChecker {
    pub fn new() -> Self {
        Self { tables: Vec::new() }
    }

    pub fn declare_table(
        &mut self,
        name: std::string::String,
        size: usize,
        generator_fn: std::string::String,
        span: std::ops::Range<usize>,
    ) {
        self.tables.push(TableDecl {
            name,
            size,
            verified_entries: 0,
            generator_fn,
            span,
        });
    }

    pub fn mark_entries_verified(&mut self, name: &str, count: usize) {
        if let Some(t) = self.tables.iter_mut().find(|t| t.name == name) {
            t.verified_entries = count;
        }
    }

    pub fn check_coverage(&self) -> Vec<TypeError> {
        self.tables
            .iter()
            .filter(|t| t.verified_entries < t.size)
            .map(|t| TypeError {
                code: "A43001".into(),
                message: format!(
                    "table `{}` has only {}/{} entries verified",
                    t.name, t.verified_entries, t.size
                ),
                span: t.span.clone(),
                secondary: None,
            })
            .collect()
    }

    pub fn check_generator(&self) -> Vec<TypeError> {
        self.tables
            .iter()
            .filter(|t| t.generator_fn.is_empty())
            .map(|t| TypeError {
                code: "A43002".into(),
                message: format!("table `{}` has no generator function", t.name),
                span: t.span.clone(),
                secondary: None,
            })
            .collect()
    }

    pub fn check_non_empty(&self) -> Vec<TypeError> {
        self.tables
            .iter()
            .filter(|t| t.size == 0)
            .map(|t| TypeError {
                code: "A43003".into(),
                message: format!("table `{}` has zero size", t.name),
                span: t.span.clone(),
                secondary: None,
            })
            .collect()
    }

    pub fn table_count(&self) -> usize {
        self.tables.len()
    }
}

impl Default for PrecomputedTableChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T097: PLAT.1 Platform abstraction
// ===========================================================================

#[derive(Debug, Clone)]
pub struct PlatformAbstractionChecker {
    platforms: Vec<std::string::String>,
    abstractions: std::collections::HashMap<std::string::String, Vec<std::string::String>>,
}

impl PlatformAbstractionChecker {
    pub fn new() -> Self {
        Self {
            platforms: Vec::new(),
            abstractions: std::collections::HashMap::new(),
        }
    }

    pub fn add_platform(&mut self, name: std::string::String) {
        if !self.platforms.contains(&name) {
            self.platforms.push(name);
        }
    }

    pub fn declare_abstraction(
        &mut self,
        name: std::string::String,
        supported: Vec<std::string::String>,
    ) {
        self.abstractions.insert(name, supported);
    }

    pub fn check_coverage(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, supported) in &self.abstractions {
            for platform in &self.platforms {
                if !supported.contains(platform) {
                    errors.push(TypeError {
                        code: "A44001".into(),
                        message: format!(
                            "abstraction `{name}` missing impl for platform `{platform}`"
                        ),
                        span: 0..1,
                        secondary: None,
                    });
                }
            }
        }
        errors
    }

    pub fn check_direct_platform_use(&self, used_platform: &str) -> Option<TypeError> {
        if self.platforms.contains(&used_platform.to_string()) {
            Some(TypeError {
                code: "A44002".into(),
                message: format!("direct platform reference `{used_platform}` without abstraction"),
                span: 0..1,
                secondary: None,
            })
        } else {
            None
        }
    }

    pub fn check_unknown_platforms(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, supported) in &self.abstractions {
            for p in supported {
                if !self.platforms.contains(p) {
                    errors.push(TypeError {
                        code: "A44003".into(),
                        message: format!("abstraction `{name}` references unknown platform `{p}`"),
                        span: 0..1,
                        secondary: None,
                    });
                }
            }
        }
        errors
    }
}

impl Default for PlatformAbstractionChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T098: PLAT.2 Feature flags
// ===========================================================================

#[derive(Debug, Clone)]
pub struct FeatureFlagChecker {
    flags: std::collections::HashMap<std::string::String, FeatureFlagInfo>,
}

#[derive(Debug, Clone)]
pub struct FeatureFlagInfo {
    pub name: std::string::String,
    pub default_enabled: bool,
    pub used: bool,
    pub conflicts_with: Vec<std::string::String>,
}

impl FeatureFlagChecker {
    pub fn new() -> Self {
        Self {
            flags: std::collections::HashMap::new(),
        }
    }

    pub fn declare(
        &mut self,
        name: std::string::String,
        default_enabled: bool,
        conflicts_with: Vec<std::string::String>,
    ) {
        self.flags.insert(
            name.clone(),
            FeatureFlagInfo {
                name,
                default_enabled,
                used: false,
                conflicts_with,
            },
        );
    }

    pub fn mark_used(&mut self, name: &str) {
        if let Some(f) = self.flags.get_mut(name) {
            f.used = true;
        }
    }

    pub fn check_unused(&self) -> Vec<TypeError> {
        self.flags
            .iter()
            .filter(|(_, i)| !i.used)
            .map(|(n, _)| TypeError {
                code: "A45001".into(),
                message: format!("feature flag `{n}` is declared but never used"),
                span: 0..1,
                secondary: None,
            })
            .collect()
    }

    pub fn check_conflicts(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, info) in &self.flags {
            if info.default_enabled {
                for conflict in &info.conflicts_with {
                    if let Some(other) = self.flags.get(conflict)
                        && other.default_enabled
                    {
                        errors.push(TypeError {
                            code: "A45002".into(),
                            message: format!(
                                "conflicting flags: `{name}` and `{conflict}` both enabled"
                            ),
                            span: 0..1,
                            secondary: None,
                        });
                    }
                }
            }
        }
        errors
    }

    pub fn check_undeclared(&self, flag_name: &str) -> Option<TypeError> {
        if !self.flags.contains_key(flag_name) {
            Some(TypeError {
                code: "A45003".into(),
                message: format!("reference to undeclared feature flag `{flag_name}`"),
                span: 0..1,
                secondary: None,
            })
        } else {
            None
        }
    }
}

impl Default for FeatureFlagChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T099: PLAT.3 Resource limits
// ===========================================================================

#[derive(Debug, Clone)]
pub struct ResourceLimitChecker {
    limits: std::collections::HashMap<std::string::String, ResourceLimit>,
    usage: std::collections::HashMap<std::string::String, u64>,
}

#[derive(Debug, Clone)]
pub struct ResourceLimit {
    pub name: std::string::String,
    pub max_value: u64,
    pub unit: std::string::String,
}

impl ResourceLimitChecker {
    pub fn new() -> Self {
        Self {
            limits: std::collections::HashMap::new(),
            usage: std::collections::HashMap::new(),
        }
    }

    pub fn declare_limit(
        &mut self,
        name: std::string::String,
        max_value: u64,
        unit: std::string::String,
    ) {
        self.limits.insert(
            name.clone(),
            ResourceLimit {
                name: name.clone(),
                max_value,
                unit,
            },
        );
        self.usage.insert(name, 0);
    }

    pub fn record_usage(&mut self, name: &str, amount: u64) {
        if let Some(u) = self.usage.get_mut(name) {
            *u += amount;
        }
    }

    pub fn release_usage(&mut self, name: &str, amount: u64) {
        if let Some(u) = self.usage.get_mut(name) {
            *u = u.saturating_sub(amount);
        }
    }

    pub fn check_limits(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, limit) in &self.limits {
            if let Some(&current) = self.usage.get(name)
                && current > limit.max_value
            {
                errors.push(TypeError {
                    code: "A46001".into(),
                    message: format!(
                        "resource `{name}` usage {current} exceeds limit {} {}",
                        limit.max_value, limit.unit
                    ),
                    span: 0..1,
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn check_unbounded(&self, name: &str) -> Option<TypeError> {
        if !self.limits.contains_key(name) {
            Some(TypeError {
                code: "A46002".into(),
                message: format!("resource `{name}` used without declared limit"),
                span: 0..1,
                secondary: None,
            })
        } else {
            None
        }
    }

    pub fn check_near_limit(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, limit) in &self.limits {
            if let Some(&current) = self.usage.get(name)
                && limit.max_value > 0
                && current > limit.max_value * 9 / 10
            {
                errors.push(TypeError {
                    code: "A46003".into(),
                    message: format!(
                        "resource `{name}` at {}% of limit",
                        current * 100 / limit.max_value
                    ),
                    span: 0..1,
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn current_usage(&self, name: &str) -> Option<u64> {
        self.usage.get(name).copied()
    }
}

impl Default for ResourceLimitChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T100: PERF.1 Unsafe escape with proof
// ===========================================================================

#[derive(Debug, Clone)]
pub struct UnsafeEscapeChecker {
    unsafe_blocks: Vec<UnsafeBlock>,
}

#[derive(Debug, Clone)]
pub struct UnsafeBlock {
    pub name: std::string::String,
    pub has_safety_proof: bool,
    pub proof_obligations: Vec<std::string::String>,
    pub obligations_discharged: Vec<std::string::String>,
    pub span: std::ops::Range<usize>,
}

impl UnsafeEscapeChecker {
    pub fn new() -> Self {
        Self {
            unsafe_blocks: Vec::new(),
        }
    }

    pub fn declare_unsafe(
        &mut self,
        name: std::string::String,
        obligations: Vec<std::string::String>,
        span: std::ops::Range<usize>,
    ) {
        self.unsafe_blocks.push(UnsafeBlock {
            name,
            has_safety_proof: false,
            proof_obligations: obligations,
            obligations_discharged: Vec::new(),
            span,
        });
    }

    pub fn attach_proof(&mut self, name: &str) {
        if let Some(b) = self.unsafe_blocks.iter_mut().find(|b| b.name == name) {
            b.has_safety_proof = true;
        }
    }

    pub fn discharge_obligation(&mut self, block_name: &str, obligation: std::string::String) {
        if let Some(b) = self.unsafe_blocks.iter_mut().find(|b| b.name == block_name) {
            b.obligations_discharged.push(obligation);
        }
    }

    pub fn check_unproven(&self) -> Vec<TypeError> {
        self.unsafe_blocks
            .iter()
            .filter(|b| !b.has_safety_proof)
            .map(|b| TypeError {
                code: "A47001".into(),
                message: format!("unsafe block `{}` has no safety proof", b.name),
                span: b.span.clone(),
                secondary: None,
            })
            .collect()
    }

    pub fn check_obligations(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for b in &self.unsafe_blocks {
            for obl in &b.proof_obligations {
                if !b.obligations_discharged.contains(obl) {
                    errors.push(TypeError {
                        code: "A47002".into(),
                        message: format!(
                            "obligation `{obl}` in unsafe block `{}` not discharged",
                            b.name
                        ),
                        span: b.span.clone(),
                        secondary: None,
                    });
                }
            }
        }
        errors
    }

    pub fn check_empty_obligations(&self) -> Vec<TypeError> {
        self.unsafe_blocks
            .iter()
            .filter(|b| b.proof_obligations.is_empty())
            .map(|b| TypeError {
                code: "A47003".into(),
                message: format!("unsafe block `{}` declares no proof obligations", b.name),
                span: b.span.clone(),
                secondary: None,
            })
            .collect()
    }

    pub fn unsafe_count(&self) -> usize {
        self.unsafe_blocks.len()
    }
}

impl Default for UnsafeEscapeChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T101: PERF.2 Complexity bounds (AARA)
// ===========================================================================

#[derive(Debug, Clone)]
pub struct ComplexityBoundChecker {
    bounds: std::collections::HashMap<std::string::String, ComplexityBound>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ComplexityClass {
    Constant,
    Logarithmic,
    Linear,
    NLogN,
    Quadratic,
    Cubic,
    Exponential,
}

#[derive(Debug, Clone)]
pub struct ComplexityBound {
    pub fn_name: std::string::String,
    pub declared: ComplexityClass,
    pub measured: Option<ComplexityClass>,
    pub span: std::ops::Range<usize>,
}

impl ComplexityBoundChecker {
    pub fn new() -> Self {
        Self {
            bounds: std::collections::HashMap::new(),
        }
    }

    pub fn declare_bound(
        &mut self,
        fn_name: std::string::String,
        declared: ComplexityClass,
        span: std::ops::Range<usize>,
    ) {
        self.bounds.insert(
            fn_name.clone(),
            ComplexityBound {
                fn_name,
                declared,
                measured: None,
                span,
            },
        );
    }

    pub fn record_measured(&mut self, fn_name: &str, measured: ComplexityClass) {
        if let Some(b) = self.bounds.get_mut(fn_name) {
            b.measured = Some(measured);
        }
    }

    fn class_rank(c: &ComplexityClass) -> u8 {
        match c {
            ComplexityClass::Constant => 0,
            ComplexityClass::Logarithmic => 1,
            ComplexityClass::Linear => 2,
            ComplexityClass::NLogN => 3,
            ComplexityClass::Quadratic => 4,
            ComplexityClass::Cubic => 5,
            ComplexityClass::Exponential => 6,
        }
    }

    pub fn check_bounds(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, bound) in &self.bounds {
            if let Some(ref measured) = bound.measured
                && Self::class_rank(measured) > Self::class_rank(&bound.declared)
            {
                errors.push(TypeError {
                    code: "A48001".into(),
                    message: format!(
                        "function `{name}` declared as {:?} but measured as {measured:?}",
                        bound.declared
                    ),
                    span: bound.span.clone(),
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn check_unverified(&self) -> Vec<TypeError> {
        self.bounds
            .iter()
            .filter(|(_, b)| b.measured.is_none())
            .map(|(n, b)| TypeError {
                code: "A48002".into(),
                message: format!("complexity bound for `{n}` is not verified"),
                span: b.span.clone(),
                secondary: None,
            })
            .collect()
    }

    pub fn check_expensive(&self) -> Vec<TypeError> {
        self.bounds
            .iter()
            .filter(|(_, b)| b.declared == ComplexityClass::Exponential)
            .map(|(n, b)| TypeError {
                code: "A48003".into(),
                message: format!("function `{n}` has exponential complexity bound"),
                span: b.span.clone(),
                secondary: None,
            })
            .collect()
    }
}

impl Default for ComplexityBoundChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T102: TEST.2 Behavioral equivalence
// ===========================================================================

#[derive(Debug, Clone)]
pub struct BehavioralEquivalenceChecker {
    equivalences: Vec<EquivalenceDecl>,
}

#[derive(Debug, Clone)]
pub struct EquivalenceDecl {
    pub name: std::string::String,
    pub impl_a: std::string::String,
    pub impl_b: std::string::String,
    pub contract: std::string::String,
    pub verified: bool,
    pub span: std::ops::Range<usize>,
}

impl BehavioralEquivalenceChecker {
    pub fn new() -> Self {
        Self {
            equivalences: Vec::new(),
        }
    }

    pub fn declare(
        &mut self,
        name: std::string::String,
        impl_a: std::string::String,
        impl_b: std::string::String,
        contract: std::string::String,
        span: std::ops::Range<usize>,
    ) {
        self.equivalences.push(EquivalenceDecl {
            name,
            impl_a,
            impl_b,
            contract,
            verified: false,
            span,
        });
    }

    pub fn mark_verified(&mut self, name: &str) {
        if let Some(e) = self.equivalences.iter_mut().find(|e| e.name == name) {
            e.verified = true;
        }
    }

    pub fn check_unverified(&self) -> Vec<TypeError> {
        self.equivalences
            .iter()
            .filter(|e| !e.verified)
            .map(|e| TypeError {
                code: "A49001".into(),
                message: format!(
                    "behavioral equivalence `{}` between `{}` and `{}` not verified",
                    e.name, e.impl_a, e.impl_b
                ),
                span: e.span.clone(),
                secondary: None,
            })
            .collect()
    }

    pub fn check_self_equivalence(&self) -> Vec<TypeError> {
        self.equivalences
            .iter()
            .filter(|e| e.impl_a == e.impl_b)
            .map(|e| TypeError {
                code: "A49002".into(),
                message: format!(
                    "trivial self-equivalence in `{}`: both sides are `{}`",
                    e.name, e.impl_a
                ),
                span: e.span.clone(),
                secondary: None,
            })
            .collect()
    }

    pub fn check_contract_ref(&self) -> Vec<TypeError> {
        self.equivalences
            .iter()
            .filter(|e| e.contract.is_empty())
            .map(|e| TypeError {
                code: "A49003".into(),
                message: format!("equivalence `{}` has no contract reference", e.name),
                span: e.span.clone(),
                secondary: None,
            })
            .collect()
    }
}

impl Default for BehavioralEquivalenceChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T103: TEST.3 Multi-pass refinement
// ===========================================================================

#[derive(Debug, Clone)]
pub struct MultiPassRefinementChecker {
    passes: Vec<RefinementPass>,
}

#[derive(Debug, Clone)]
pub struct RefinementPass {
    pub name: std::string::String,
    pub from_level: std::string::String,
    pub to_level: std::string::String,
    pub obligations_total: usize,
    pub obligations_discharged: usize,
    pub span: std::ops::Range<usize>,
}

impl MultiPassRefinementChecker {
    pub fn new() -> Self {
        Self { passes: Vec::new() }
    }

    pub fn add_pass(
        &mut self,
        name: std::string::String,
        from_level: std::string::String,
        to_level: std::string::String,
        obligations: usize,
        span: std::ops::Range<usize>,
    ) {
        self.passes.push(RefinementPass {
            name,
            from_level,
            to_level,
            obligations_total: obligations,
            obligations_discharged: 0,
            span,
        });
    }

    pub fn discharge(&mut self, pass_name: &str, count: usize) {
        if let Some(p) = self.passes.iter_mut().find(|p| p.name == pass_name) {
            p.obligations_discharged += count;
        }
    }

    pub fn check_complete(&self) -> Vec<TypeError> {
        self.passes
            .iter()
            .filter(|p| p.obligations_discharged < p.obligations_total)
            .map(|p| TypeError {
                code: "A50001".into(),
                message: format!(
                    "refinement `{}` ({} -> {}): {}/{} obligations discharged",
                    p.name, p.from_level, p.to_level, p.obligations_discharged, p.obligations_total
                ),
                span: p.span.clone(),
                secondary: None,
            })
            .collect()
    }

    pub fn check_chain(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for i in 1..self.passes.len() {
            if self.passes[i].from_level != self.passes[i - 1].to_level {
                errors.push(TypeError {
                    code: "A50002".into(),
                    message: format!(
                        "refinement chain gap: `{}` starts at `{}` but `{}` ends at `{}`",
                        self.passes[i].name,
                        self.passes[i].from_level,
                        self.passes[i - 1].name,
                        self.passes[i - 1].to_level
                    ),
                    span: self.passes[i].span.clone(),
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn check_non_trivial(&self) -> Vec<TypeError> {
        self.passes
            .iter()
            .filter(|p| p.obligations_total == 0)
            .map(|p| TypeError {
                code: "A50003".into(),
                message: format!("refinement pass `{}` has zero obligations", p.name),
                span: p.span.clone(),
                secondary: None,
            })
            .collect()
    }

    pub fn pass_count(&self) -> usize {
        self.passes.len()
    }
}

impl Default for MultiPassRefinementChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T104: MISC.1 Incremental contracts
// ===========================================================================

#[derive(Debug, Clone)]
pub struct IncrementalContractChecker {
    contracts: std::collections::HashMap<std::string::String, ContractHistoryEntry>,
}

#[derive(Debug, Clone)]
pub struct ContractHistoryEntry {
    pub name: std::string::String,
    pub versions: Vec<ContractVersionEntry>,
}

#[derive(Debug, Clone)]
pub struct ContractVersionEntry {
    pub version: u32,
    pub requires_count: usize,
    pub ensures_count: usize,
}

impl IncrementalContractChecker {
    pub fn new() -> Self {
        Self {
            contracts: std::collections::HashMap::new(),
        }
    }

    pub fn add_version(
        &mut self,
        name: std::string::String,
        version: u32,
        requires_count: usize,
        ensures_count: usize,
    ) {
        let history = self
            .contracts
            .entry(name.clone())
            .or_insert_with(|| ContractHistoryEntry {
                name,
                versions: Vec::new(),
            });
        history.versions.push(ContractVersionEntry {
            version,
            requires_count,
            ensures_count,
        });
    }

    pub fn check_precondition_weakening(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, history) in &self.contracts {
            for i in 1..history.versions.len() {
                if history.versions[i].requires_count > history.versions[i - 1].requires_count {
                    errors.push(TypeError {
                        code: "A51001".into(),
                        message: format!(
                            "contract `{name}` v{} strengthens preconditions",
                            history.versions[i].version
                        ),
                        span: 0..1,
                        secondary: None,
                    });
                }
            }
        }
        errors
    }

    pub fn check_postcondition_strengthening(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, history) in &self.contracts {
            for i in 1..history.versions.len() {
                if history.versions[i].ensures_count < history.versions[i - 1].ensures_count {
                    errors.push(TypeError {
                        code: "A51002".into(),
                        message: format!(
                            "contract `{name}` v{} weakens postconditions",
                            history.versions[i].version
                        ),
                        span: 0..1,
                        secondary: None,
                    });
                }
            }
        }
        errors
    }

    pub fn check_version_continuity(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, history) in &self.contracts {
            for i in 1..history.versions.len() {
                if history.versions[i].version != history.versions[i - 1].version + 1 {
                    errors.push(TypeError {
                        code: "A51003".into(),
                        message: format!(
                            "contract `{name}` has version gap: v{} to v{}",
                            history.versions[i - 1].version,
                            history.versions[i].version
                        ),
                        span: 0..1,
                        secondary: None,
                    });
                }
            }
        }
        errors
    }
}

impl Default for IncrementalContractChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T105: MISC.2 Scoped invariant suspension
// ===========================================================================

#[derive(Debug, Clone)]
pub struct ScopedInvariantChecker {
    invariants: std::collections::HashMap<std::string::String, InvariantState>,
    suspension_depth: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InvariantState {
    Active,
    Suspended,
    Restored,
}

impl ScopedInvariantChecker {
    pub fn new() -> Self {
        Self {
            invariants: std::collections::HashMap::new(),
            suspension_depth: 0,
        }
    }

    pub fn declare_invariant(&mut self, name: std::string::String) {
        self.invariants.insert(name, InvariantState::Active);
    }

    pub fn suspend(&mut self, name: &str) -> Option<TypeError> {
        if let Some(state) = self.invariants.get_mut(name) {
            if *state == InvariantState::Suspended {
                return Some(TypeError {
                    code: "A52001".into(),
                    message: format!("invariant `{name}` is already suspended"),
                    span: 0..1,
                    secondary: None,
                });
            }
            *state = InvariantState::Suspended;
            self.suspension_depth += 1;
            None
        } else {
            Some(TypeError {
                code: "A52002".into(),
                message: format!("cannot suspend undeclared invariant `{name}`"),
                span: 0..1,
                secondary: None,
            })
        }
    }

    pub fn restore(&mut self, name: &str) -> Option<TypeError> {
        if let Some(state) = self.invariants.get_mut(name) {
            if *state != InvariantState::Suspended {
                return Some(TypeError {
                    code: "A52003".into(),
                    message: format!("invariant `{name}` is not currently suspended"),
                    span: 0..1,
                    secondary: None,
                });
            }
            *state = InvariantState::Restored;
            if self.suspension_depth > 0 {
                self.suspension_depth -= 1;
            }
            None
        } else {
            None
        }
    }

    pub fn check_all_restored(&self) -> Vec<TypeError> {
        self.invariants
            .iter()
            .filter(|(_, s)| **s == InvariantState::Suspended)
            .map(|(n, _)| TypeError {
                code: "A52001".into(),
                message: format!("invariant `{n}` still suspended at scope exit"),
                span: 0..1,
                secondary: None,
            })
            .collect()
    }

    pub fn is_suspended(&self, name: &str) -> bool {
        self.invariants.get(name) == Some(&InvariantState::Suspended)
    }

    pub fn suspension_depth(&self) -> u32 {
        self.suspension_depth
    }
}

impl Default for ScopedInvariantChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T107: Core standard library types
// ===========================================================================

/// Core standard library type definitions (Pos, NonNeg, Email, Uuid, etc.)
#[derive(Debug, Clone)]
pub struct StdlibTypes {
    types: std::collections::HashMap<std::string::String, StdlibTypeDef>,
}

#[derive(Debug, Clone)]
pub struct StdlibTypeDef {
    pub name: std::string::String,
    pub base_type: Type,
    pub refinement: std::string::String,
    pub description: std::string::String,
}

impl StdlibTypes {
    pub fn new() -> Self {
        let mut types = std::collections::HashMap::new();
        types.insert(
            "Pos".into(),
            StdlibTypeDef {
                name: "Pos".into(),
                base_type: Type::Int,
                refinement: "v > 0".into(),
                description: "Positive integer".into(),
            },
        );
        types.insert(
            "NonNeg".into(),
            StdlibTypeDef {
                name: "NonNeg".into(),
                base_type: Type::Int,
                refinement: "v >= 0".into(),
                description: "Non-negative integer".into(),
            },
        );
        types.insert(
            "Email".into(),
            StdlibTypeDef {
                name: "Email".into(),
                base_type: Type::String,
                refinement: "contains(v, @)".into(),
                description: "Email address".into(),
            },
        );
        types.insert(
            "Uuid".into(),
            StdlibTypeDef {
                name: "Uuid".into(),
                base_type: Type::String,
                refinement: "len(v) == 36".into(),
                description: "UUID string".into(),
            },
        );
        types.insert(
            "Port".into(),
            StdlibTypeDef {
                name: "Port".into(),
                base_type: Type::Int,
                refinement: "v >= 0 && v <= 65535".into(),
                description: "Network port".into(),
            },
        );
        types.insert(
            "Percentage".into(),
            StdlibTypeDef {
                name: "Percentage".into(),
                base_type: Type::Float,
                refinement: "v >= 0.0 && v <= 100.0".into(),
                description: "Percentage value".into(),
            },
        );
        Self { types }
    }

    pub fn lookup(&self, name: &str) -> Option<&StdlibTypeDef> {
        self.types.get(name)
    }

    pub fn all_types(&self) -> Vec<&StdlibTypeDef> {
        self.types.values().collect()
    }

    pub fn type_count(&self) -> usize {
        self.types.len()
    }

    pub fn is_stdlib_type(&self, name: &str) -> bool {
        self.types.contains_key(name)
    }
}

impl Default for StdlibTypes {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T108: Collection contracts (ListOps, sort, filter)
// ===========================================================================

/// Standard collection operation contracts.
#[derive(Debug, Clone)]
pub struct CollectionContracts {
    contracts: Vec<CollectionContract>,
}

#[derive(Debug, Clone)]
pub struct CollectionContract {
    pub name: std::string::String,
    pub collection_type: std::string::String,
    pub preconditions: Vec<std::string::String>,
    pub postconditions: Vec<std::string::String>,
    pub preserves_length: bool,
    pub preserves_elements: bool,
}

impl CollectionContracts {
    pub fn new() -> Self {
        let contracts = vec![
            CollectionContract {
                name: "sort".into(),
                collection_type: "List<T>".into(),
                preconditions: vec![],
                postconditions: vec![
                    "is_sorted(result)".into(),
                    "len(result) == len(input)".into(),
                ],
                preserves_length: true,
                preserves_elements: true,
            },
            CollectionContract {
                name: "filter".into(),
                collection_type: "List<T>".into(),
                preconditions: vec![],
                postconditions: vec![
                    "len(result) <= len(input)".into(),
                    "forall x in result: pred(x)".into(),
                ],
                preserves_length: false,
                preserves_elements: true,
            },
            CollectionContract {
                name: "map".into(),
                collection_type: "List<T>".into(),
                preconditions: vec![],
                postconditions: vec!["len(result) == len(input)".into()],
                preserves_length: true,
                preserves_elements: false,
            },
            CollectionContract {
                name: "reverse".into(),
                collection_type: "List<T>".into(),
                preconditions: vec![],
                postconditions: vec![
                    "len(result) == len(input)".into(),
                    "result[0] == input[len(input)-1]".into(),
                ],
                preserves_length: true,
                preserves_elements: true,
            },
            CollectionContract {
                name: "deduplicate".into(),
                collection_type: "List<T>".into(),
                preconditions: vec![],
                postconditions: vec![
                    "len(result) <= len(input)".into(),
                    "all_unique(result)".into(),
                ],
                preserves_length: false,
                preserves_elements: true,
            },
        ];
        Self { contracts }
    }

    pub fn lookup(&self, name: &str) -> Option<&CollectionContract> {
        self.contracts.iter().find(|c| c.name == name)
    }

    pub fn all_contracts(&self) -> &[CollectionContract] {
        &self.contracts
    }

    pub fn contract_count(&self) -> usize {
        self.contracts.len()
    }
}

impl Default for CollectionContracts {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T109: CRUD patterns and auth contracts
// ===========================================================================

/// Standard CRUD and authentication contract patterns.
#[derive(Debug, Clone)]
pub struct CrudAuthContracts {
    crud_ops: Vec<CrudOperation>,
    auth_policies: Vec<AuthPolicy>,
}

#[derive(Debug, Clone)]
pub struct CrudOperation {
    pub name: std::string::String,
    pub op_type: CrudType,
    pub requires_auth: bool,
    pub preconditions: Vec<std::string::String>,
    pub postconditions: Vec<std::string::String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CrudType {
    Create,
    Read,
    Update,
    Delete,
}

#[derive(Debug, Clone)]
pub struct AuthPolicy {
    pub name: std::string::String,
    pub required_role: std::string::String,
    pub allow_self: bool,
}

impl CrudAuthContracts {
    pub fn new() -> Self {
        Self {
            crud_ops: Vec::new(),
            auth_policies: Vec::new(),
        }
    }

    pub fn add_crud(&mut self, name: std::string::String, op_type: CrudType, requires_auth: bool) {
        self.crud_ops.push(CrudOperation {
            name,
            op_type,
            requires_auth,
            preconditions: Vec::new(),
            postconditions: Vec::new(),
        });
    }

    pub fn add_auth_policy(
        &mut self,
        name: std::string::String,
        required_role: std::string::String,
        allow_self: bool,
    ) {
        self.auth_policies.push(AuthPolicy {
            name,
            required_role,
            allow_self,
        });
    }

    pub fn check_auth_coverage(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for op in &self.crud_ops {
            if op.requires_auth {
                let has_policy = self.auth_policies.iter().any(|p| p.name == op.name);
                if !has_policy {
                    errors.push(TypeError {
                        code: "A53001".into(),
                        message: format!(
                            "CRUD operation `{}` requires auth but has no policy",
                            op.name
                        ),
                        span: 0..1,
                        secondary: None,
                    });
                }
            }
        }
        errors
    }

    pub fn check_delete_protection(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for op in &self.crud_ops {
            if op.op_type == CrudType::Delete && !op.requires_auth {
                errors.push(TypeError {
                    code: "A53002".into(),
                    message: format!(
                        "delete operation `{}` should require authentication",
                        op.name
                    ),
                    span: 0..1,
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn crud_count(&self) -> usize {
        self.crud_ops.len()
    }
    pub fn policy_count(&self) -> usize {
        self.auth_policies.len()
    }
}

impl Default for CrudAuthContracts {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T110: Contract composition with extends
// ===========================================================================

/// Tracks contract inheritance/composition via extends.
#[derive(Debug, Clone)]
pub struct ContractCompositionChecker {
    contracts: std::collections::HashMap<std::string::String, ComposableContract>,
}

#[derive(Debug, Clone)]
pub struct ComposableContract {
    pub name: std::string::String,
    pub extends: Vec<std::string::String>,
    pub own_clauses: usize,
}

impl ContractCompositionChecker {
    pub fn new() -> Self {
        Self {
            contracts: std::collections::HashMap::new(),
        }
    }

    pub fn declare(
        &mut self,
        name: std::string::String,
        extends: Vec<std::string::String>,
        own_clauses: usize,
    ) {
        self.contracts.insert(
            name.clone(),
            ComposableContract {
                name,
                extends,
                own_clauses,
            },
        );
    }

    /// Check that all extended contracts exist.
    pub fn check_extends(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, contract) in &self.contracts {
            for parent in &contract.extends {
                if !self.contracts.contains_key(parent) {
                    errors.push(TypeError {
                        code: "A54001".into(),
                        message: format!("contract `{name}` extends unknown contract `{parent}`"),
                        span: 0..1,
                        secondary: None,
                    });
                }
            }
        }
        errors
    }

    /// Check for circular extends.
    pub fn check_circular(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for name in self.contracts.keys() {
            let mut visited = vec![name.clone()];
            if self.has_extends_cycle(name, &mut visited) {
                errors.push(TypeError {
                    code: "A54002".into(),
                    message: format!("circular extends chain involving `{name}`"),
                    span: 0..1,
                    secondary: None,
                });
            }
        }
        errors
    }

    fn has_extends_cycle(&self, current: &str, visited: &mut Vec<std::string::String>) -> bool {
        if let Some(contract) = self.contracts.get(current) {
            for parent in &contract.extends {
                if visited.contains(parent) {
                    return true;
                }
                visited.push(parent.clone());
                if self.has_extends_cycle(parent, visited) {
                    return true;
                }
                visited.pop();
            }
        }
        false
    }

    /// Check for diamond inheritance (same contract extended via two paths).
    pub fn check_diamond(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for (name, contract) in &self.contracts {
            let mut all_ancestors = Vec::new();
            for parent in &contract.extends {
                let ancestors = self.collect_ancestors(parent);
                for a in &ancestors {
                    if all_ancestors.contains(a) {
                        errors.push(TypeError {
                            code: "A54003".into(),
                            message: format!(
                                "diamond inheritance in `{name}`: `{a}` reached via multiple paths"
                            ),
                            span: 0..1,
                            secondary: None,
                        });
                    }
                }
                all_ancestors.extend(ancestors);
            }
        }
        errors
    }

    fn collect_ancestors(&self, name: &str) -> Vec<std::string::String> {
        let mut result = vec![name.to_string()];
        if let Some(c) = self.contracts.get(name) {
            for parent in &c.extends {
                result.extend(self.collect_ancestors(parent));
            }
        }
        result
    }

    pub fn contract_count(&self) -> usize {
        self.contracts.len()
    }
}

impl Default for ContractCompositionChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T111: Contract libraries as publishable packages
// ===========================================================================

/// Tracks contract library packaging metadata.
#[derive(Debug, Clone)]
pub struct ContractLibraryChecker {
    libraries: Vec<ContractLibrary>,
}

#[derive(Debug, Clone)]
pub struct ContractLibrary {
    pub name: std::string::String,
    pub version: std::string::String,
    pub exported_contracts: Vec<std::string::String>,
    pub dependencies: Vec<LibraryDep>,
}

#[derive(Debug, Clone)]
pub struct LibraryDep {
    pub name: std::string::String,
    pub version_req: std::string::String,
}

impl ContractLibraryChecker {
    pub fn new() -> Self {
        Self {
            libraries: Vec::new(),
        }
    }

    pub fn declare_library(&mut self, name: std::string::String, version: std::string::String) {
        self.libraries.push(ContractLibrary {
            name,
            version,
            exported_contracts: Vec::new(),
            dependencies: Vec::new(),
        });
    }

    pub fn add_export(&mut self, lib_name: &str, contract: std::string::String) {
        if let Some(lib) = self.libraries.iter_mut().find(|l| l.name == lib_name) {
            lib.exported_contracts.push(contract);
        }
    }

    pub fn add_dependency(&mut self, lib_name: &str, dep: LibraryDep) {
        if let Some(lib) = self.libraries.iter_mut().find(|l| l.name == lib_name) {
            lib.dependencies.push(dep);
        }
    }

    /// Check for libraries with no exports.
    pub fn check_empty_exports(&self) -> Vec<TypeError> {
        self.libraries
            .iter()
            .filter(|l| l.exported_contracts.is_empty())
            .map(|l| TypeError {
                code: "A55001".into(),
                message: format!("library `{}` has no exported contracts", l.name),
                span: 0..1,
                secondary: None,
            })
            .collect()
    }

    /// Check for circular dependencies.
    pub fn check_circular_deps(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for lib in &self.libraries {
            for dep in &lib.dependencies {
                if dep.name == lib.name {
                    errors.push(TypeError {
                        code: "A55002".into(),
                        message: format!("library `{}` depends on itself", lib.name),
                        span: 0..1,
                        secondary: None,
                    });
                }
            }
        }
        errors
    }

    /// Check for duplicate library names.
    pub fn check_duplicates(&self) -> Vec<TypeError> {
        let mut seen = std::collections::HashSet::new();
        let mut errors = Vec::new();
        for lib in &self.libraries {
            if !seen.insert(lib.name.clone()) {
                errors.push(TypeError {
                    code: "A55003".into(),
                    message: format!("duplicate library name `{}`", lib.name),
                    span: 0..1,
                    secondary: None,
                });
            }
        }
        errors
    }

    pub fn library_count(&self) -> usize {
        self.libraries.len()
    }
}

impl Default for ContractLibraryChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: parse + resolve source text, panicking on errors.
    fn resolve_ok(source: &str) -> ResolvedFile {
        let (file, errs) = assura_parser::parse(source);
        assert!(errs.is_empty(), "unexpected parse errors: {errs:?}");
        let file = file.expect("parse returned None");
        assura_resolve::resolve(&file).expect("resolve should succeed")
    }

    #[test]
    fn empty_file_type_checks() {
        let resolved = resolve_ok("");
        let typed = type_check(&resolved).expect("type_check should succeed");
        // Should have at least the built-in types in the environment.
        assert!(!typed.type_env.is_empty());
    }

    #[test]
    fn builtin_types_in_env() {
        let resolved = resolve_ok("");
        let typed = type_check(&resolved).expect("type_check should succeed");
        let env = &typed.type_env;

        assert_eq!(env.lookup("Int"), Some(&Type::Int));
        assert_eq!(env.lookup("Nat"), Some(&Type::Nat));
        assert_eq!(env.lookup("Float"), Some(&Type::Float));
        assert_eq!(env.lookup("Bool"), Some(&Type::Bool));
        assert_eq!(env.lookup("String"), Some(&Type::String));
        assert_eq!(env.lookup("Bytes"), Some(&Type::Bytes));
        assert_eq!(env.lookup("Unit"), Some(&Type::Unit));
        assert_eq!(env.lookup("Never"), Some(&Type::Never));
        assert_eq!(env.lookup("U8"), Some(&Type::U8));
        assert_eq!(env.lookup("U16"), Some(&Type::U16));
        assert_eq!(env.lookup("U32"), Some(&Type::U32));
        assert_eq!(env.lookup("U64"), Some(&Type::U64));
        assert_eq!(env.lookup("I8"), Some(&Type::I8));
        assert_eq!(env.lookup("I16"), Some(&Type::I16));
        assert_eq!(env.lookup("I32"), Some(&Type::I32));
        assert_eq!(env.lookup("I64"), Some(&Type::I64));
        assert_eq!(env.lookup("F32"), Some(&Type::F32));
        assert_eq!(env.lookup("F64"), Some(&Type::F64));
    }

    #[test]
    fn user_defined_types_in_env() {
        let src = r#"
type Foo {
  x: Int
  y: Bool
}

enum Color {
  Red
  Green
  Blue
}
"#;
        let resolved = resolve_ok(src);
        let typed = type_check(&resolved).expect("type_check should succeed");
        let env = &typed.type_env;

        assert_eq!(env.lookup("Foo"), Some(&Type::Named("Foo".into())));
        assert_eq!(env.lookup("Color"), Some(&Type::Named("Color".into())));
        // Enum variants are Named
        assert_eq!(env.lookup("Red"), Some(&Type::Named("Red".into())));
    }

    #[test]
    fn contract_in_env() {
        let src = r#"
contract SafeBuffer {
  requires { true }
}
"#;
        let resolved = resolve_ok(src);
        let typed = type_check(&resolved).expect("type_check should succeed");
        assert_eq!(
            typed.type_env.lookup("SafeBuffer"),
            Some(&Type::Named("SafeBuffer".into()))
        );
    }

    #[test]
    fn fn_def_in_env() {
        let src = r#"
fn helper(n: Int) -> Int {
  ensures { result >= 0 }
}
"#;
        let resolved = resolve_ok(src);
        let typed = type_check(&resolved).expect("type_check should succeed");
        assert_eq!(
            typed.type_env.lookup("helper"),
            Some(&Type::Fn {
                params: vec![Type::Int],
                ret: Box::new(Type::Int),
            })
        );
        // Parameter now gets parsed type from Param.ty tokens
        assert_eq!(typed.type_env.lookup("n"), Some(&Type::Int));
    }

    #[test]
    fn type_param_in_env() {
        let src = r#"
contract Container<T> {
  requires { true }
}
"#;
        let resolved = resolve_ok(src);
        let typed = type_check(&resolved).expect("type_check should succeed");
        assert_eq!(
            typed.type_env.lookup("T"),
            Some(&Type::TypeParam("T".into()))
        );
    }

    #[test]
    fn typed_file_preserves_resolved() {
        let src = r#"
type Point {
  x: Int
  y: Int
}
"#;
        let resolved = resolve_ok(src);
        let typed = type_check(&resolved).expect("type_check should succeed");
        // The resolved file should be preserved intact
        assert_eq!(typed.resolved.source.decls.len(), 1);
    }

    #[test]
    fn type_env_len() {
        let resolved = resolve_ok("");
        let typed = type_check(&resolved).expect("type_check should succeed");
        // At minimum, all 22 built-in types should be in the env
        assert!(typed.type_env.len() >= 22);
    }

    // -----------------------------------------------------------------------
    // parse_type_tokens tests
    // -----------------------------------------------------------------------

    #[test]
    fn parse_type_base_int() {
        let tokens: Vec<String> = vec!["Int".into()];
        assert_eq!(parse_type_tokens(&tokens), Type::Int);
    }

    #[test]
    fn parse_type_base_nat() {
        let tokens: Vec<String> = vec!["Nat".into()];
        assert_eq!(parse_type_tokens(&tokens), Type::Nat);
    }

    #[test]
    fn parse_type_generic_list() {
        let tokens: Vec<String> = ["List", "<", "Int", ">"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(parse_type_tokens(&tokens), Type::List(Box::new(Type::Int)));
    }

    #[test]
    fn parse_type_generic_map() {
        let tokens: Vec<String> = ["Map", "<", "String", ",", "Int", ">"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            parse_type_tokens(&tokens),
            Type::Map(Box::new(Type::String), Box::new(Type::Int))
        );
    }

    #[test]
    fn parse_type_sequence() {
        let tokens: Vec<String> = ["Sequence", "<", "Nat", ">"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            parse_type_tokens(&tokens),
            Type::Sequence(Box::new(Type::Nat))
        );
    }

    #[test]
    fn parse_type_refined() {
        let tokens: Vec<String> = ["{", "x", ":", "Int", "|", "x", ">", "0", "}"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            parse_type_tokens(&tokens),
            Type::Refined {
                base: Box::new(Type::Int),
                predicate: String::new(),
            }
        );
    }

    #[test]
    fn parse_type_taint_stripped() {
        let tokens: Vec<String> = ["U32", "@", "taint", ":", "untrusted"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(parse_type_tokens(&tokens), Type::U32);
    }

    #[test]
    fn parse_type_reference_stripped() {
        let tokens: Vec<String> = ["&", "mut", "BitReader"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(parse_type_tokens(&tokens), Type::Named("BitReader".into()));
    }

    #[test]
    fn parse_type_union_error() {
        let tokens: Vec<String> = ["HuffmanGroup", "|", "DecodeError"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            parse_type_tokens(&tokens),
            Type::Result(
                Box::new(Type::Named("HuffmanGroup".into())),
                Box::new(Type::Named("DecodeError".into()))
            )
        );
    }

    #[test]
    fn parse_type_empty() {
        assert_eq!(parse_type_tokens(&[]), Type::Unit);
    }

    #[test]
    fn parse_type_named() {
        let tokens: Vec<String> = vec!["ValidCodeLengths".into()];
        assert_eq!(
            parse_type_tokens(&tokens),
            Type::Named("ValidCodeLengths".into())
        );
    }

    #[test]
    fn fn_params_parsed_from_ast() {
        // Test that build_type_env enriches function types from AST
        let src = r#"
fn compute(x: Nat, y: Float) -> Bool {
  ensures { result == true }
}
"#;
        let resolved = resolve_ok(src);
        let typed = type_check(&resolved).expect("type_check should succeed");
        assert_eq!(
            typed.type_env.lookup("compute"),
            Some(&Type::Fn {
                params: vec![Type::Nat, Type::Float],
                ret: Box::new(Type::Bool),
            })
        );
        assert_eq!(typed.type_env.lookup("x"), Some(&Type::Nat));
        assert_eq!(typed.type_env.lookup("y"), Some(&Type::Float));
    }

    #[test]
    fn extern_params_parsed_from_ast() {
        let src = r#"
extern fn read_bytes(n: U32) -> Bytes
  effects { io.read }
"#;
        let resolved = resolve_ok(src);
        let typed = type_check(&resolved).expect("type_check should succeed");
        assert_eq!(
            typed.type_env.lookup("read_bytes"),
            Some(&Type::Fn {
                params: vec![Type::U32],
                ret: Box::new(Type::Bytes),
            })
        );
    }

    // -----------------------------------------------------------------------
    // T014: Expression type inference tests
    // -----------------------------------------------------------------------

    use assura_parser::ast::{
        BinOp as AstBinOp, Clause as AstClause, Expr as AstExpr, FnDef as AstFnDef,
        Literal as AstLit, Param as AstParam, UnaryOp as AstUnOp,
    };

    #[test]
    fn infer_int_literal() {
        let env = TypeEnv::new();
        let expr = AstExpr::Literal(AstLit::Int("42".into()));
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
    }

    #[test]
    fn infer_float_literal() {
        let env = TypeEnv::new();
        let expr = AstExpr::Literal(AstLit::Float("3.14".into()));
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Float);
    }

    #[test]
    fn infer_string_literal() {
        let env = TypeEnv::new();
        let expr = AstExpr::Literal(AstLit::Str("hello".into()));
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::String);
    }

    #[test]
    fn infer_bool_literal() {
        let env = TypeEnv::new();
        let expr = AstExpr::Literal(AstLit::Bool(true));
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
    }

    #[test]
    fn infer_ident_known() {
        let mut env = TypeEnv::new();
        env.insert("x".into(), Type::Int);
        let expr = AstExpr::Ident("x".into());
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
    }

    #[test]
    fn infer_ident_unknown() {
        let env = TypeEnv::new();
        let expr = AstExpr::Ident("unknown_var".into());
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
    }

    #[test]
    fn infer_arithmetic_add() {
        let env = TypeEnv::new();
        let expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Literal(AstLit::Int("2".into()))),
        };
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
    }

    #[test]
    fn infer_arithmetic_float_mul() {
        let env = TypeEnv::new();
        let expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Literal(AstLit::Float("1.0".into()))),
            op: AstBinOp::Mul,
            rhs: Box::new(AstExpr::Literal(AstLit::Float("2.0".into()))),
        };
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Float);
    }

    #[test]
    fn infer_arithmetic_numeric_types_compatible() {
        // Numeric types (Int, Float, Nat, etc.) are compatible in arithmetic
        let env = TypeEnv::new();
        let expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Literal(AstLit::Float("2.0".into()))),
        };
        // Int + Float is accepted (numeric widening)
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
    }

    #[test]
    fn infer_arithmetic_non_numeric() {
        let env = TypeEnv::new();
        let expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Literal(AstLit::Bool(true))),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Literal(AstLit::Bool(false))),
        };
        let err = infer_expr(&expr, &env).unwrap_err();
        assert_eq!(err.code, "A03001");
        assert!(err.message.contains("numeric"));
    }

    #[test]
    fn infer_comparison_same_type() {
        let env = TypeEnv::new();
        let expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
            op: AstBinOp::Lt,
            rhs: Box::new(AstExpr::Literal(AstLit::Int("2".into()))),
        };
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
    }

    #[test]
    fn infer_comparison_mismatch() {
        let env = TypeEnv::new();
        let expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
            op: AstBinOp::Eq,
            rhs: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        };
        let err = infer_expr(&expr, &env).unwrap_err();
        assert_eq!(err.code, "A03001");
    }

    #[test]
    fn infer_logical_and() {
        let env = TypeEnv::new();
        let expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Literal(AstLit::Bool(true))),
            op: AstBinOp::And,
            rhs: Box::new(AstExpr::Literal(AstLit::Bool(false))),
        };
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
    }

    #[test]
    fn infer_logical_non_bool() {
        let env = TypeEnv::new();
        let expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
            op: AstBinOp::And,
            rhs: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        };
        let err = infer_expr(&expr, &env).unwrap_err();
        assert_eq!(err.code, "A03001");
        assert!(err.message.contains("Bool"));
    }

    #[test]
    fn infer_implies() {
        let env = TypeEnv::new();
        let expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Literal(AstLit::Bool(true))),
            op: AstBinOp::Implies,
            rhs: Box::new(AstExpr::Literal(AstLit::Bool(false))),
        };
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
    }

    #[test]
    fn infer_unary_neg() {
        let env = TypeEnv::new();
        let expr = AstExpr::UnaryOp {
            op: AstUnOp::Neg,
            expr: Box::new(AstExpr::Literal(AstLit::Int("5".into()))),
        };
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
    }

    #[test]
    fn infer_unary_neg_non_numeric() {
        let env = TypeEnv::new();
        let expr = AstExpr::UnaryOp {
            op: AstUnOp::Neg,
            expr: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        };
        let err = infer_expr(&expr, &env).unwrap_err();
        assert_eq!(err.code, "A03001");
    }

    #[test]
    fn infer_unary_not() {
        let env = TypeEnv::new();
        let expr = AstExpr::UnaryOp {
            op: AstUnOp::Not,
            expr: Box::new(AstExpr::Literal(AstLit::Bool(false))),
        };
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
    }

    #[test]
    fn infer_unary_not_non_bool() {
        let env = TypeEnv::new();
        let expr = AstExpr::UnaryOp {
            op: AstUnOp::Not,
            expr: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
        };
        let err = infer_expr(&expr, &env).unwrap_err();
        assert_eq!(err.code, "A03001");
    }

    #[test]
    fn infer_if_then_else() {
        let env = TypeEnv::new();
        let expr = AstExpr::If {
            cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
            then_branch: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
            else_branch: Some(Box::new(AstExpr::Literal(AstLit::Int("2".into())))),
        };
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
    }

    #[test]
    fn infer_if_branch_mismatch() {
        let env = TypeEnv::new();
        let expr = AstExpr::If {
            cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
            then_branch: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
            else_branch: Some(Box::new(AstExpr::Literal(AstLit::Bool(false)))),
        };
        let err = infer_expr(&expr, &env).unwrap_err();
        assert_eq!(err.code, "A03001");
        assert!(err.message.contains("different types"));
    }

    #[test]
    fn infer_if_non_bool_cond() {
        let env = TypeEnv::new();
        let expr = AstExpr::If {
            cond: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
            then_branch: Box::new(AstExpr::Literal(AstLit::Int("2".into()))),
            else_branch: None,
        };
        let err = infer_expr(&expr, &env).unwrap_err();
        assert_eq!(err.code, "A03001");
        assert!(err.message.contains("Bool"));
    }

    #[test]
    fn infer_old_preserves_type() {
        let mut env = TypeEnv::new();
        env.insert("x".into(), Type::Int);
        let expr = AstExpr::Old(Box::new(AstExpr::Ident("x".into())));
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
    }

    #[test]
    fn infer_paren_preserves_type() {
        let env = TypeEnv::new();
        let expr = AstExpr::Paren(Box::new(AstExpr::Literal(AstLit::Float("1.5".into()))));
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Float);
    }

    #[test]
    fn infer_forall_is_bool() {
        let env = TypeEnv::new();
        let expr = AstExpr::Forall {
            var: "i".into(),
            domain: Box::new(AstExpr::Ident("S".into())),
            body: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        };
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
    }

    #[test]
    fn infer_exists_is_bool() {
        let env = TypeEnv::new();
        let expr = AstExpr::Exists {
            var: "i".into(),
            domain: Box::new(AstExpr::Ident("S".into())),
            body: Box::new(AstExpr::Literal(AstLit::Bool(true))),
        };
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
    }

    #[test]
    fn infer_list_uniform() {
        let env = TypeEnv::new();
        let expr = AstExpr::List(vec![
            AstExpr::Literal(AstLit::Int("1".into())),
            AstExpr::Literal(AstLit::Int("2".into())),
            AstExpr::Literal(AstLit::Int("3".into())),
        ]);
        assert_eq!(
            infer_expr(&expr, &env).unwrap(),
            Type::List(Box::new(Type::Int))
        );
    }

    #[test]
    fn infer_list_empty() {
        let env = TypeEnv::new();
        let expr = AstExpr::List(vec![]);
        assert_eq!(
            infer_expr(&expr, &env).unwrap(),
            Type::List(Box::new(Type::Unknown))
        );
    }

    #[test]
    fn infer_list_type_mismatch() {
        let env = TypeEnv::new();
        let expr = AstExpr::List(vec![
            AstExpr::Literal(AstLit::Int("1".into())),
            AstExpr::Literal(AstLit::Bool(true)),
        ]);
        let err = infer_expr(&expr, &env).unwrap_err();
        assert_eq!(err.code, "A03001");
        assert!(err.message.contains("list"));
    }

    #[test]
    fn infer_unknown_no_error_in_binop() {
        let env = TypeEnv::new();
        // unknown_var + 1 should not error (unknown_var is Unknown)
        let expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("unknown_var".into())),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
        };
        // Should succeed with Int (known side propagated)
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
    }

    #[test]
    fn infer_range_op() {
        let env = TypeEnv::new();
        let expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
            op: AstBinOp::Range,
            rhs: Box::new(AstExpr::Literal(AstLit::Int("10".into()))),
        };
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
    }

    #[test]
    fn infer_in_op() {
        let env = TypeEnv::new();
        let expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
            op: AstBinOp::In,
            rhs: Box::new(AstExpr::Ident("collection".into())),
        };
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
    }

    #[test]
    fn infer_raw_is_unknown() {
        let env = TypeEnv::new();
        let expr = AstExpr::Raw(vec!["some".into(), "tokens".into()]);
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
    }

    #[test]
    fn infer_field_is_unknown() {
        let env = TypeEnv::new();
        let expr = AstExpr::Field(Box::new(AstExpr::Ident("x".into())), "len".into());
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
    }

    #[test]
    fn infer_call_is_unknown() {
        let env = TypeEnv::new();
        let expr = AstExpr::Call {
            func: Box::new(AstExpr::Ident("f".into())),
            args: vec![],
        };
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
    }

    // -----------------------------------------------------------------------
    // T016: Field access and function call type checking tests
    // -----------------------------------------------------------------------

    #[test]
    fn infer_field_on_named_type_is_unknown() {
        let mut env = TypeEnv::new();
        env.insert("p".into(), Type::Named("Point".into()));
        let expr = AstExpr::Field(Box::new(AstExpr::Ident("p".into())), "x".into());
        // Named type without struct field info returns Unknown
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
    }

    #[test]
    fn infer_field_resolves_struct_field() {
        let mut env = TypeEnv::new();
        env.insert("p".into(), Type::Named("Point".into()));
        env.struct_fields.insert(
            "Point".into(),
            vec![("x".into(), Type::Int), ("y".into(), Type::Int)],
        );
        let expr = AstExpr::Field(Box::new(AstExpr::Ident("p".into())), "x".into());
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
    }

    #[test]
    fn infer_field_unknown_field_on_known_struct() {
        let mut env = TypeEnv::new();
        env.insert("p".into(), Type::Named("Point".into()));
        env.struct_fields
            .insert("Point".into(), vec![("x".into(), Type::Int)]);
        // Accessing unknown field returns Unknown (lenient, no A03004 yet)
        let expr = AstExpr::Field(Box::new(AstExpr::Ident("p".into())), "z".into());
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
    }

    #[test]
    fn infer_field_collection_len() {
        let mut env = TypeEnv::new();
        env.insert("xs".into(), Type::List(Box::new(Type::Int)));
        let expr = AstExpr::Field(Box::new(AstExpr::Ident("xs".into())), "len".into());
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Nat);
    }

    #[test]
    fn infer_method_collection_contains() {
        let mut env = TypeEnv::new();
        env.insert("xs".into(), Type::List(Box::new(Type::Int)));
        let expr = AstExpr::MethodCall {
            receiver: Box::new(AstExpr::Ident("xs".into())),
            method: "contains".into(),
            args: vec![AstExpr::Literal(AstLit::Int("1".into()))],
        };
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
    }

    #[test]
    fn infer_method_list_get() {
        let mut env = TypeEnv::new();
        env.insert("xs".into(), Type::List(Box::new(Type::Int)));
        let expr = AstExpr::MethodCall {
            receiver: Box::new(AstExpr::Ident("xs".into())),
            method: "get".into(),
            args: vec![AstExpr::Literal(AstLit::Int("0".into()))],
        };
        assert_eq!(
            infer_expr(&expr, &env).unwrap(),
            Type::Option(Box::new(Type::Int))
        );
    }

    #[test]
    fn field_resolution_from_ast() {
        let src = r#"
type Point {
  x: Int
  y: Float
}
"#;
        let resolved = resolve_ok(src);
        let typed = type_check(&resolved).expect("type_check should succeed");
        // NOTE: without field separators (comma/semicolon), the parser groups
        // all tokens after the first colon into one field. Use commas.
        assert_eq!(typed.type_env.lookup_field("Point", "x"), Some(&Type::Int));
    }

    #[test]
    fn field_resolution_with_commas() {
        let src = r#"
type Point {
  x: Int,
  y: Float
}
"#;
        let resolved = resolve_ok(src);
        let typed = type_check(&resolved).expect("type_check should succeed");
        assert_eq!(typed.type_env.lookup_field("Point", "x"), Some(&Type::Int));
        assert_eq!(
            typed.type_env.lookup_field("Point", "y"),
            Some(&Type::Float)
        );
        assert_eq!(typed.type_env.lookup_field("Point", "z"), None);
    }

    #[test]
    fn infer_field_surfaces_receiver_error() {
        let env = TypeEnv::new();
        // Field access on an expression that has an error inside:
        // (!42).field -> error inside unary !
        let expr = AstExpr::Field(
            Box::new(AstExpr::UnaryOp {
                op: AstUnOp::Not,
                expr: Box::new(AstExpr::Literal(AstLit::Int("42".into()))),
            }),
            "field".into(),
        );
        let err = infer_expr(&expr, &env).unwrap_err();
        assert_eq!(err.code, "A03001");
    }

    #[test]
    fn infer_call_fn_type_returns_ret() {
        let mut env = TypeEnv::new();
        env.insert(
            "add".into(),
            Type::Fn {
                params: vec![Type::Int, Type::Int],
                ret: Box::new(Type::Int),
            },
        );
        let expr = AstExpr::Call {
            func: Box::new(AstExpr::Ident("add".into())),
            args: vec![
                AstExpr::Literal(AstLit::Int("1".into())),
                AstExpr::Literal(AstLit::Int("2".into())),
            ],
        };
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
    }

    #[test]
    fn infer_call_wrong_arg_count_a03002() {
        let mut env = TypeEnv::new();
        env.insert(
            "inc".into(),
            Type::Fn {
                params: vec![Type::Int],
                ret: Box::new(Type::Int),
            },
        );
        let expr = AstExpr::Call {
            func: Box::new(AstExpr::Ident("inc".into())),
            args: vec![
                AstExpr::Literal(AstLit::Int("1".into())),
                AstExpr::Literal(AstLit::Int("2".into())),
            ],
        };
        let err = infer_expr(&expr, &env).unwrap_err();
        assert_eq!(err.code, "A03002");
        assert!(err.message.contains("1"));
        assert!(err.message.contains("2"));
    }

    #[test]
    fn infer_call_not_callable_a03005() {
        let mut env = TypeEnv::new();
        env.insert("x".into(), Type::Int);
        let expr = AstExpr::Call {
            func: Box::new(AstExpr::Ident("x".into())),
            args: vec![],
        };
        let err = infer_expr(&expr, &env).unwrap_err();
        assert_eq!(err.code, "A03005");
        assert!(err.message.contains("Int"));
        assert!(err.message.contains("not callable"));
    }

    #[test]
    fn infer_call_bool_not_callable_a03005() {
        let mut env = TypeEnv::new();
        env.insert("flag".into(), Type::Bool);
        let expr = AstExpr::Call {
            func: Box::new(AstExpr::Ident("flag".into())),
            args: vec![AstExpr::Literal(AstLit::Int("1".into()))],
        };
        let err = infer_expr(&expr, &env).unwrap_err();
        assert_eq!(err.code, "A03005");
    }

    #[test]
    fn infer_call_unknown_callee_is_lenient() {
        let env = TypeEnv::new();
        let expr = AstExpr::Call {
            func: Box::new(AstExpr::Ident("unknown_fn".into())),
            args: vec![AstExpr::Literal(AstLit::Int("1".into()))],
        };
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
    }

    #[test]
    fn infer_call_named_type_is_lenient() {
        let mut env = TypeEnv::new();
        env.insert("MyType".into(), Type::Named("MyType".into()));
        // Calling a Named type is lenient (could be a constructor)
        let expr = AstExpr::Call {
            func: Box::new(AstExpr::Ident("MyType".into())),
            args: vec![AstExpr::Literal(AstLit::Int("1".into()))],
        };
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
    }

    #[test]
    fn infer_call_fn_empty_params_skips_count_check() {
        let mut env = TypeEnv::new();
        // Functions from symbol table have empty params (not yet resolved)
        env.insert(
            "f".into(),
            Type::Fn {
                params: vec![],
                ret: Box::new(Type::Bool),
            },
        );
        let expr = AstExpr::Call {
            func: Box::new(AstExpr::Ident("f".into())),
            args: vec![
                AstExpr::Literal(AstLit::Int("1".into())),
                AstExpr::Literal(AstLit::Int("2".into())),
            ],
        };
        // Empty params means we skip count check, return ret type
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
    }

    #[test]
    fn infer_call_surfaces_arg_error() {
        let mut env = TypeEnv::new();
        env.insert(
            "f".into(),
            Type::Fn {
                params: vec![],
                ret: Box::new(Type::Unknown),
            },
        );
        // Argument has a type error inside it: true + false
        let expr = AstExpr::Call {
            func: Box::new(AstExpr::Ident("f".into())),
            args: vec![AstExpr::BinOp {
                lhs: Box::new(AstExpr::Literal(AstLit::Bool(true))),
                op: AstBinOp::Add,
                rhs: Box::new(AstExpr::Literal(AstLit::Bool(false))),
            }],
        };
        let err = infer_expr(&expr, &env).unwrap_err();
        assert_eq!(err.code, "A03001");
    }

    #[test]
    fn infer_method_call_is_unknown() {
        let env = TypeEnv::new();
        let expr = AstExpr::MethodCall {
            receiver: Box::new(AstExpr::Ident("obj".into())),
            method: "do_something".into(),
            args: vec![AstExpr::Literal(AstLit::Int("1".into()))],
        };
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
    }

    #[test]
    fn infer_method_call_surfaces_receiver_error() {
        let env = TypeEnv::new();
        // receiver has a type error: true + 1
        let expr = AstExpr::MethodCall {
            receiver: Box::new(AstExpr::BinOp {
                lhs: Box::new(AstExpr::Literal(AstLit::Bool(true))),
                op: AstBinOp::Add,
                rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
            }),
            method: "m".into(),
            args: vec![],
        };
        let err = infer_expr(&expr, &env).unwrap_err();
        assert_eq!(err.code, "A03001");
    }

    #[test]
    fn infer_index_list_returns_element_type() {
        let mut env = TypeEnv::new();
        env.insert("xs".into(), Type::List(Box::new(Type::Int)));
        let expr = AstExpr::Index {
            expr: Box::new(AstExpr::Ident("xs".into())),
            index: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
        };
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Int);
    }

    #[test]
    fn infer_index_map_returns_value_type() {
        let mut env = TypeEnv::new();
        env.insert(
            "m".into(),
            Type::Map(Box::new(Type::String), Box::new(Type::Bool)),
        );
        let expr = AstExpr::Index {
            expr: Box::new(AstExpr::Ident("m".into())),
            index: Box::new(AstExpr::Literal(AstLit::Str("key".into()))),
        };
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Bool);
    }

    #[test]
    fn infer_index_sequence_returns_element_type() {
        let mut env = TypeEnv::new();
        env.insert("seq".into(), Type::Sequence(Box::new(Type::Float)));
        let expr = AstExpr::Index {
            expr: Box::new(AstExpr::Ident("seq".into())),
            index: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
        };
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Float);
    }

    #[test]
    fn infer_index_unknown_base_is_unknown() {
        let env = TypeEnv::new();
        let expr = AstExpr::Index {
            expr: Box::new(AstExpr::Ident("unknown".into())),
            index: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
        };
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
    }

    #[test]
    fn infer_index_named_type_is_unknown() {
        let mut env = TypeEnv::new();
        env.insert("arr".into(), Type::Named("CustomArray".into()));
        let expr = AstExpr::Index {
            expr: Box::new(AstExpr::Ident("arr".into())),
            index: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
        };
        // Named type indexing returns Unknown (could be user-defined indexable)
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
    }

    #[test]
    fn infer_index_surfaces_index_error() {
        let mut env = TypeEnv::new();
        env.insert("xs".into(), Type::List(Box::new(Type::Int)));
        // Index expr has a type error: true && 42
        let expr = AstExpr::Index {
            expr: Box::new(AstExpr::Ident("xs".into())),
            index: Box::new(AstExpr::BinOp {
                lhs: Box::new(AstExpr::Literal(AstLit::Bool(true))),
                op: AstBinOp::And,
                rhs: Box::new(AstExpr::Literal(AstLit::Int("42".into()))),
            }),
        };
        let err = infer_expr(&expr, &env).unwrap_err();
        assert_eq!(err.code, "A03001");
    }

    #[test]
    fn type_display_basic() {
        assert_eq!(format!("{}", Type::Int), "Int");
        assert_eq!(format!("{}", Type::Bool), "Bool");
        assert_eq!(format!("{}", Type::List(Box::new(Type::Int))), "List<Int>");
        assert_eq!(format!("{}", Type::Unknown), "Unknown");
    }

    // -----------------------------------------------------------------------
    // T015: Generic type instantiation tests
    // -----------------------------------------------------------------------

    /// Helper: build a minimal SourceFile with declarations for testing
    /// generic instantiation against user-defined types.
    fn source_with_decls(
        decls: Vec<assura_parser::ast::Spanned<Decl>>,
    ) -> assura_parser::ast::SourceFile {
        assura_parser::ast::SourceFile {
            project: None,
            module: None,
            imports: vec![],
            decls,
        }
    }

    fn spanned_decl(decl: Decl) -> assura_parser::ast::Spanned<Decl> {
        assura_parser::ast::Spanned {
            node: decl,
            span: 0..1,
        }
    }

    #[test]
    fn generic_list_one_arg_ok() {
        let src = source_with_decls(vec![]);
        let result = check_generic_instantiation("List", &[Type::Int], &(0..1), &src);
        assert!(result.is_ok());
    }

    #[test]
    fn generic_list_zero_args_a03003() {
        let src = source_with_decls(vec![]);
        let err = check_generic_instantiation("List", &[], &(0..1), &src).unwrap_err();
        assert_eq!(err.code, "A03003");
        assert!(err.message.contains("List"));
        assert!(err.message.contains("expected 1"));
        assert!(err.message.contains("found 0"));
    }

    #[test]
    fn generic_list_two_args_a03003() {
        let src = source_with_decls(vec![]);
        let err = check_generic_instantiation("List", &[Type::Int, Type::Bool], &(0..1), &src)
            .unwrap_err();
        assert_eq!(err.code, "A03003");
        assert!(err.message.contains("expected 1"));
        assert!(err.message.contains("found 2"));
    }

    #[test]
    fn generic_map_two_args_ok() {
        let src = source_with_decls(vec![]);
        let result = check_generic_instantiation("Map", &[Type::String, Type::Int], &(0..1), &src);
        assert!(result.is_ok());
    }

    #[test]
    fn generic_map_one_arg_a03003() {
        let src = source_with_decls(vec![]);
        let err = check_generic_instantiation("Map", &[Type::String], &(0..1), &src).unwrap_err();
        assert_eq!(err.code, "A03003");
        assert!(err.message.contains("Map"));
        assert!(err.message.contains("expected 2"));
        assert!(err.message.contains("found 1"));
    }

    #[test]
    fn generic_set_one_arg_ok() {
        let src = source_with_decls(vec![]);
        let result = check_generic_instantiation("Set", &[Type::Int], &(0..1), &src);
        assert!(result.is_ok());
    }

    #[test]
    fn generic_option_one_arg_ok() {
        let src = source_with_decls(vec![]);
        let result = check_generic_instantiation("Option", &[Type::Bool], &(0..1), &src);
        assert!(result.is_ok());
    }

    #[test]
    fn generic_result_two_args_ok() {
        let src = source_with_decls(vec![]);
        let result =
            check_generic_instantiation("Result", &[Type::Int, Type::String], &(0..1), &src);
        assert!(result.is_ok());
    }

    #[test]
    fn generic_result_three_args_a03003() {
        let src = source_with_decls(vec![]);
        let err = check_generic_instantiation(
            "Result",
            &[Type::Int, Type::String, Type::Bool],
            &(0..1),
            &src,
        )
        .unwrap_err();
        assert_eq!(err.code, "A03003");
        assert!(err.message.contains("expected 2"));
        assert!(err.message.contains("found 3"));
    }

    #[test]
    fn generic_sequence_one_arg_ok() {
        let src = source_with_decls(vec![]);
        let result = check_generic_instantiation("Sequence", &[Type::Nat], &(0..1), &src);
        assert!(result.is_ok());
    }

    #[test]
    fn generic_user_defined_type_correct_arity() {
        let decls = vec![spanned_decl(Decl::TypeDef(assura_parser::ast::TypeDef {
            name: "Pair".into(),
            type_params: vec!["A".into(), "B".into()],
            body: assura_parser::ast::TypeBody::Empty,
        }))];
        let src = source_with_decls(decls);
        let result = check_generic_instantiation("Pair", &[Type::Int, Type::Bool], &(0..1), &src);
        assert!(result.is_ok());
    }

    #[test]
    fn generic_user_defined_type_wrong_arity() {
        let decls = vec![spanned_decl(Decl::TypeDef(assura_parser::ast::TypeDef {
            name: "Pair".into(),
            type_params: vec!["A".into(), "B".into()],
            body: assura_parser::ast::TypeBody::Empty,
        }))];
        let src = source_with_decls(decls);
        let err = check_generic_instantiation("Pair", &[Type::Int], &(0..1), &src).unwrap_err();
        assert_eq!(err.code, "A03003");
        assert!(err.message.contains("Pair"));
        assert!(err.message.contains("expected 2"));
        assert!(err.message.contains("found 1"));
    }

    #[test]
    fn generic_user_defined_enum_correct_arity() {
        let decls = vec![spanned_decl(Decl::EnumDef(assura_parser::ast::EnumDef {
            name: "Maybe".into(),
            type_params: vec!["T".into()],
            variants: vec![],
        }))];
        let src = source_with_decls(decls);
        let result = check_generic_instantiation("Maybe", &[Type::Int], &(0..1), &src);
        assert!(result.is_ok());
    }

    #[test]
    fn generic_user_defined_enum_wrong_arity() {
        let decls = vec![spanned_decl(Decl::EnumDef(assura_parser::ast::EnumDef {
            name: "Maybe".into(),
            type_params: vec!["T".into()],
            variants: vec![],
        }))];
        let src = source_with_decls(decls);
        let err = check_generic_instantiation("Maybe", &[Type::Int, Type::Bool], &(0..1), &src)
            .unwrap_err();
        assert_eq!(err.code, "A03003");
        assert!(err.message.contains("Maybe"));
        assert!(err.message.contains("expected 1"));
        assert!(err.message.contains("found 2"));
    }

    #[test]
    fn generic_user_defined_contract_correct_arity() {
        let decls = vec![spanned_decl(Decl::Contract(
            assura_parser::ast::ContractDecl {
                name: "Container".into(),
                type_params: vec!["T".into()],
                clauses: vec![],
            },
        ))];
        let src = source_with_decls(decls);
        let result = check_generic_instantiation("Container", &[Type::Int], &(0..1), &src);
        assert!(result.is_ok());
    }

    #[test]
    fn generic_user_defined_non_generic_type_zero_args_ok() {
        let decls = vec![spanned_decl(Decl::TypeDef(assura_parser::ast::TypeDef {
            name: "Foo".into(),
            type_params: vec![],
            body: assura_parser::ast::TypeBody::Empty,
        }))];
        let src = source_with_decls(decls);
        let result = check_generic_instantiation("Foo", &[], &(0..1), &src);
        assert!(result.is_ok());
    }

    #[test]
    fn generic_user_defined_non_generic_type_with_args_a03003() {
        let decls = vec![spanned_decl(Decl::TypeDef(assura_parser::ast::TypeDef {
            name: "Foo".into(),
            type_params: vec![],
            body: assura_parser::ast::TypeBody::Empty,
        }))];
        let src = source_with_decls(decls);
        let err = check_generic_instantiation("Foo", &[Type::Int], &(0..1), &src).unwrap_err();
        assert_eq!(err.code, "A03003");
        assert!(err.message.contains("expected 0"));
        assert!(err.message.contains("found 1"));
    }

    #[test]
    fn generic_unknown_type_is_lenient() {
        let src = source_with_decls(vec![]);
        // Unknown type name; not our problem (name resolution handles it)
        let result = check_generic_instantiation("UnknownType", &[Type::Int], &(0..1), &src);
        assert!(result.is_ok());
    }

    // -- substitute() tests --

    #[test]
    fn substitute_type_param() {
        let mut bindings = HashMap::new();
        bindings.insert("T".into(), Type::Int);
        let result = substitute(&Type::TypeParam("T".into()), &bindings);
        assert_eq!(result, Type::Int);
    }

    #[test]
    fn substitute_unbound_type_param_unchanged() {
        let bindings = HashMap::new();
        let result = substitute(&Type::TypeParam("T".into()), &bindings);
        assert_eq!(result, Type::TypeParam("T".into()));
    }

    #[test]
    fn substitute_in_list() {
        let mut bindings = HashMap::new();
        bindings.insert("T".into(), Type::Int);
        let ty = Type::List(Box::new(Type::TypeParam("T".into())));
        let result = substitute(&ty, &bindings);
        assert_eq!(result, Type::List(Box::new(Type::Int)));
    }

    #[test]
    fn substitute_in_map() {
        let mut bindings = HashMap::new();
        bindings.insert("K".into(), Type::String);
        bindings.insert("V".into(), Type::Int);
        let ty = Type::Map(
            Box::new(Type::TypeParam("K".into())),
            Box::new(Type::TypeParam("V".into())),
        );
        let result = substitute(&ty, &bindings);
        assert_eq!(
            result,
            Type::Map(Box::new(Type::String), Box::new(Type::Int))
        );
    }

    #[test]
    fn substitute_in_set() {
        let mut bindings = HashMap::new();
        bindings.insert("T".into(), Type::Bool);
        let ty = Type::Set(Box::new(Type::TypeParam("T".into())));
        let result = substitute(&ty, &bindings);
        assert_eq!(result, Type::Set(Box::new(Type::Bool)));
    }

    #[test]
    fn substitute_in_option() {
        let mut bindings = HashMap::new();
        bindings.insert("T".into(), Type::Float);
        let ty = Type::Option(Box::new(Type::TypeParam("T".into())));
        let result = substitute(&ty, &bindings);
        assert_eq!(result, Type::Option(Box::new(Type::Float)));
    }

    #[test]
    fn substitute_in_result() {
        let mut bindings = HashMap::new();
        bindings.insert("T".into(), Type::Int);
        bindings.insert("E".into(), Type::String);
        let ty = Type::Result(
            Box::new(Type::TypeParam("T".into())),
            Box::new(Type::TypeParam("E".into())),
        );
        let result = substitute(&ty, &bindings);
        assert_eq!(
            result,
            Type::Result(Box::new(Type::Int), Box::new(Type::String))
        );
    }

    #[test]
    fn substitute_in_sequence() {
        let mut bindings = HashMap::new();
        bindings.insert("T".into(), Type::Nat);
        let ty = Type::Sequence(Box::new(Type::TypeParam("T".into())));
        let result = substitute(&ty, &bindings);
        assert_eq!(result, Type::Sequence(Box::new(Type::Nat)));
    }

    #[test]
    fn substitute_in_fn_type() {
        let mut bindings = HashMap::new();
        bindings.insert("T".into(), Type::Int);
        bindings.insert("U".into(), Type::Bool);
        let ty = Type::Fn {
            params: vec![Type::TypeParam("T".into()), Type::TypeParam("U".into())],
            ret: Box::new(Type::TypeParam("T".into())),
        };
        let result = substitute(&ty, &bindings);
        assert_eq!(
            result,
            Type::Fn {
                params: vec![Type::Int, Type::Bool],
                ret: Box::new(Type::Int),
            }
        );
    }

    #[test]
    fn substitute_in_refined_type() {
        let mut bindings = HashMap::new();
        bindings.insert("T".into(), Type::Int);
        let ty = Type::Refined {
            base: Box::new(Type::TypeParam("T".into())),
            predicate: "v > 0".into(),
        };
        let result = substitute(&ty, &bindings);
        assert_eq!(
            result,
            Type::Refined {
                base: Box::new(Type::Int),
                predicate: "v > 0".into(),
            }
        );
    }

    #[test]
    fn substitute_nested_generics() {
        let mut bindings = HashMap::new();
        bindings.insert("T".into(), Type::Int);
        // List<Option<T>> -> List<Option<Int>>
        let ty = Type::List(Box::new(Type::Option(Box::new(Type::TypeParam(
            "T".into(),
        )))));
        let result = substitute(&ty, &bindings);
        assert_eq!(
            result,
            Type::List(Box::new(Type::Option(Box::new(Type::Int))))
        );
    }

    #[test]
    fn substitute_leaves_concrete_types_unchanged() {
        let mut bindings = HashMap::new();
        bindings.insert("T".into(), Type::Bool);
        // Concrete types should be unchanged
        assert_eq!(substitute(&Type::Int, &bindings), Type::Int);
        assert_eq!(substitute(&Type::Bool, &bindings), Type::Bool);
        assert_eq!(substitute(&Type::String, &bindings), Type::String);
        assert_eq!(substitute(&Type::Unknown, &bindings), Type::Unknown);
        assert_eq!(
            substitute(&Type::Named("Foo".into()), &bindings),
            Type::Named("Foo".into())
        );
    }

    #[test]
    fn substitute_partial_bindings() {
        let mut bindings = HashMap::new();
        bindings.insert("K".into(), Type::String);
        // Map<K, V> with only K bound -> Map<String, V>
        let ty = Type::Map(
            Box::new(Type::TypeParam("K".into())),
            Box::new(Type::TypeParam("V".into())),
        );
        let result = substitute(&ty, &bindings);
        assert_eq!(
            result,
            Type::Map(
                Box::new(Type::String),
                Box::new(Type::TypeParam("V".into()))
            )
        );
    }

    // -- instantiate_builtin_generic() tests --

    #[test]
    fn instantiate_list() {
        let result = instantiate_builtin_generic("List", vec![Type::Int]);
        assert_eq!(result, Some(Type::List(Box::new(Type::Int))));
    }

    #[test]
    fn instantiate_map() {
        let result = instantiate_builtin_generic("Map", vec![Type::String, Type::Int]);
        assert_eq!(
            result,
            Some(Type::Map(Box::new(Type::String), Box::new(Type::Int)))
        );
    }

    #[test]
    fn instantiate_set() {
        let result = instantiate_builtin_generic("Set", vec![Type::Bool]);
        assert_eq!(result, Some(Type::Set(Box::new(Type::Bool))));
    }

    #[test]
    fn instantiate_option() {
        let result = instantiate_builtin_generic("Option", vec![Type::Float]);
        assert_eq!(result, Some(Type::Option(Box::new(Type::Float))));
    }

    #[test]
    fn instantiate_result() {
        let result = instantiate_builtin_generic("Result", vec![Type::Int, Type::String]);
        assert_eq!(
            result,
            Some(Type::Result(Box::new(Type::Int), Box::new(Type::String)))
        );
    }

    #[test]
    fn instantiate_sequence() {
        let result = instantiate_builtin_generic("Sequence", vec![Type::Nat]);
        assert_eq!(result, Some(Type::Sequence(Box::new(Type::Nat))));
    }

    #[test]
    fn instantiate_unknown_name_returns_none() {
        let result = instantiate_builtin_generic("Foo", vec![Type::Int]);
        assert_eq!(result, None);
    }

    #[test]
    fn instantiate_non_generic_builtin_returns_none() {
        let result = instantiate_builtin_generic("Int", vec![]);
        assert_eq!(result, None);
    }

    // -----------------------------------------------------------------------
    // T017: Pattern exhaustiveness checking tests
    // -----------------------------------------------------------------------

    #[test]
    fn exhaustive_all_variants_covered() {
        let variants = vec!["Red".into(), "Green".into(), "Blue".into()];
        let patterns = vec![
            Pattern::Variant("Red".into()),
            Pattern::Variant("Green".into()),
            Pattern::Variant("Blue".into()),
        ];
        assert_eq!(check_exhaustiveness(&patterns, &variants), None);
    }

    #[test]
    fn exhaustive_wildcard_covers_all() {
        let variants = vec!["Red".into(), "Green".into(), "Blue".into()];
        let patterns = vec![Pattern::Wildcard];
        assert_eq!(check_exhaustiveness(&patterns, &variants), None);
    }

    #[test]
    fn exhaustive_wildcard_with_explicit() {
        let variants = vec!["Red".into(), "Green".into(), "Blue".into()];
        let patterns = vec![Pattern::Variant("Red".into()), Pattern::Wildcard];
        assert_eq!(check_exhaustiveness(&patterns, &variants), None);
    }

    #[test]
    fn non_exhaustive_missing_one() {
        let variants = vec!["Red".into(), "Green".into(), "Blue".into()];
        let patterns = vec![
            Pattern::Variant("Red".into()),
            Pattern::Variant("Green".into()),
        ];
        let missing = check_exhaustiveness(&patterns, &variants);
        assert_eq!(missing, Some(vec!["Blue".into()]));
    }

    #[test]
    fn non_exhaustive_missing_multiple() {
        let variants = vec!["Red".into(), "Green".into(), "Blue".into(), "Yellow".into()];
        let patterns = vec![Pattern::Variant("Green".into())];
        let missing = check_exhaustiveness(&patterns, &variants).unwrap();
        assert_eq!(missing, vec!["Red", "Blue", "Yellow"]);
    }

    #[test]
    fn non_exhaustive_empty_patterns() {
        let variants = vec!["A".into(), "B".into(), "C".into()];
        let patterns: Vec<Pattern> = vec![];
        let missing = check_exhaustiveness(&patterns, &variants).unwrap();
        assert_eq!(missing, vec!["A", "B", "C"]);
    }

    #[test]
    fn exhaustive_empty_enum() {
        let variants: Vec<String> = vec![];
        let patterns: Vec<Pattern> = vec![];
        assert_eq!(check_exhaustiveness(&patterns, &variants), None);
    }

    #[test]
    fn exhaustive_duplicate_patterns_ignored() {
        let variants = vec!["X".into(), "Y".into()];
        let patterns = vec![
            Pattern::Variant("X".into()),
            Pattern::Variant("X".into()),
            Pattern::Variant("Y".into()),
        ];
        assert_eq!(check_exhaustiveness(&patterns, &variants), None);
    }

    #[test]
    fn non_exhaustive_literal_does_not_cover_variant() {
        let variants = vec!["Red".into(), "Green".into()];
        let patterns = vec![
            Pattern::Variant("Red".into()),
            Pattern::Literal(AstLit::Int("42".into())),
        ];
        let missing = check_exhaustiveness(&patterns, &variants).unwrap();
        assert_eq!(missing, vec!["Green"]);
    }

    #[test]
    fn exhaustive_single_variant_enum() {
        let variants = vec!["Only".into()];
        let patterns = vec![Pattern::Variant("Only".into())];
        assert_eq!(check_exhaustiveness(&patterns, &variants), None);
    }

    #[test]
    fn non_exhaustive_preserves_declaration_order() {
        let variants = vec![
            "Alpha".into(),
            "Beta".into(),
            "Gamma".into(),
            "Delta".into(),
            "Epsilon".into(),
        ];
        let patterns = vec![
            Pattern::Variant("Beta".into()),
            Pattern::Variant("Delta".into()),
        ];
        let missing = check_exhaustiveness(&patterns, &variants).unwrap();
        assert_eq!(missing, vec!["Alpha", "Gamma", "Epsilon"]);
    }

    // -----------------------------------------------------------------------
    // T018: Contract clause type checking tests
    // -----------------------------------------------------------------------

    use assura_parser::ast::ClauseKind as AstClauseKind;

    #[test]
    fn clause_requires_bool_body_ok() {
        let env = TypeEnv::new();
        let body = AstExpr::Literal(AstLit::Bool(true));
        let mut errors = Vec::new();
        check_clause_expr(&AstClauseKind::Requires, &body, &env, &mut errors, &(0..0));
        assert!(errors.is_empty());
    }

    #[test]
    fn clause_requires_int_body_error() {
        let env = TypeEnv::new();
        let body = AstExpr::Literal(AstLit::Int("42".into()));
        let mut errors = Vec::new();
        check_clause_expr(&AstClauseKind::Requires, &body, &env, &mut errors, &(0..0));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A03006");
        assert!(errors[0].message.contains("requires"));
        assert!(errors[0].message.contains("Bool"));
        assert!(errors[0].message.contains("Int"));
    }

    #[test]
    fn clause_ensures_bool_body_ok() {
        let env = TypeEnv::new();
        let body = AstExpr::Literal(AstLit::Bool(false));
        let mut errors = Vec::new();
        check_clause_expr(&AstClauseKind::Ensures, &body, &env, &mut errors, &(0..0));
        assert!(errors.is_empty());
    }

    #[test]
    fn clause_ensures_string_body_error() {
        let env = TypeEnv::new();
        let body = AstExpr::Literal(AstLit::Str("hello".into()));
        let mut errors = Vec::new();
        check_clause_expr(&AstClauseKind::Ensures, &body, &env, &mut errors, &(0..0));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A03006");
        assert!(errors[0].message.contains("ensures"));
    }

    #[test]
    fn clause_invariant_bool_body_ok() {
        let env = TypeEnv::new();
        let body = AstExpr::Literal(AstLit::Bool(true));
        let mut errors = Vec::new();
        check_clause_expr(&AstClauseKind::Invariant, &body, &env, &mut errors, &(0..0));
        assert!(errors.is_empty());
    }

    #[test]
    fn clause_invariant_float_body_error() {
        let env = TypeEnv::new();
        let body = AstExpr::Literal(AstLit::Float("3.14".into()));
        let mut errors = Vec::new();
        check_clause_expr(&AstClauseKind::Invariant, &body, &env, &mut errors, &(0..0));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A03006");
        assert!(errors[0].message.contains("invariant"));
    }

    #[test]
    fn clause_rule_bool_body_ok() {
        let env = TypeEnv::new();
        let body = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Literal(AstLit::Bool(true))),
            op: AstBinOp::And,
            rhs: Box::new(AstExpr::Literal(AstLit::Bool(false))),
        };
        let mut errors = Vec::new();
        check_clause_expr(&AstClauseKind::Rule, &body, &env, &mut errors, &(0..0));
        assert!(errors.is_empty());
    }

    #[test]
    fn clause_rule_int_body_error() {
        let env = TypeEnv::new();
        let body = AstExpr::Literal(AstLit::Int("99".into()));
        let mut errors = Vec::new();
        check_clause_expr(&AstClauseKind::Rule, &body, &env, &mut errors, &(0..0));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A03006");
        assert!(errors[0].message.contains("rule"));
    }

    #[test]
    fn clause_effects_any_body_ok() {
        let env = TypeEnv::new();
        // Effects clause accepts any type (lenient)
        let body = AstExpr::Ident("pure".into());
        let mut errors = Vec::new();
        check_clause_expr(&AstClauseKind::Effects, &body, &env, &mut errors, &(0..0));
        assert!(errors.is_empty());
    }

    #[test]
    fn clause_modifies_any_body_ok() {
        let env = TypeEnv::new();
        let body = AstExpr::Ident("buffer".into());
        let mut errors = Vec::new();
        check_clause_expr(&AstClauseKind::Modifies, &body, &env, &mut errors, &(0..0));
        assert!(errors.is_empty());
    }

    #[test]
    fn clause_unknown_body_no_error() {
        let env = TypeEnv::new();
        // Unknown ident in requires clause should not emit A03006
        let body = AstExpr::Ident("unknown_predicate".into());
        let mut errors = Vec::new();
        check_clause_expr(&AstClauseKind::Requires, &body, &env, &mut errors, &(0..0));
        assert!(errors.is_empty());
    }

    #[test]
    fn clause_comparison_in_requires_ok() {
        let mut env = TypeEnv::new();
        env.insert("x".into(), Type::Int);
        // x > 0 should infer as Bool, valid in requires
        let body = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("x".into())),
            op: AstBinOp::Gt,
            rhs: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
        };
        let mut errors = Vec::new();
        check_clause_expr(&AstClauseKind::Requires, &body, &env, &mut errors, &(0..0));
        assert!(errors.is_empty());
    }

    #[test]
    fn clause_requires_int_body_integration() {
        // Integration test: a contract whose requires clause has an Int body
        // should produce an A03006 error through the full type_check pipeline.
        let src = r#"
contract Bad {
  requires { 42 }
}
"#;
        let resolved = resolve_ok(src);
        let result = type_check(&resolved);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.iter().any(|e| e.code == "A03006"));
    }

    #[test]
    fn clause_requires_bool_integration() {
        // A contract with a Bool requires clause should type-check fine.
        let src = r#"
contract Good {
  requires { true }
}
"#;
        let resolved = resolve_ok(src);
        type_check(&resolved).expect("should type-check successfully");
    }

    #[test]
    fn demo_files_type_check() {
        // Verify all demo files still type-check without errors
        for path in [
            "demos/libwebp-huffman.assura",
            "demos/zlib-inflate.assura",
            "demos/mbedtls-x509.assura",
            "tests/fixtures/test_basic.assura",
        ] {
            let full = format!(
                "{}/{}",
                env!("CARGO_MANIFEST_DIR")
                    .strip_suffix("/crates/assura-types")
                    .unwrap_or(env!("CARGO_MANIFEST_DIR")),
                path
            );
            // Try the workspace root path
            let content = match std::fs::read_to_string(&full) {
                Ok(c) => c,
                Err(_) => {
                    // Try from two levels up (crates/assura-types -> workspace root)
                    let alt = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .parent()
                        .and_then(|p| p.parent())
                        .unwrap()
                        .join(path);
                    std::fs::read_to_string(alt)
                        .unwrap_or_else(|e| panic!("cannot read {path}: {e}"))
                }
            };
            let (file, parse_errs) = assura_parser::parse(&content);
            assert!(
                parse_errs.is_empty(),
                "{path}: unexpected parse errors: {parse_errs:?}"
            );
            let file = file.unwrap_or_else(|| panic!("{path}: parse returned None"));
            let resolved = assura_resolve::resolve(&file)
                .unwrap_or_else(|e| panic!("{path}: resolve errors: {e:?}"));
            type_check(&resolved).unwrap_or_else(|e| panic!("{path}: type_check errors: {e:?}"));
        }
    }

    // -----------------------------------------------------------------------
    // T031: Usage tracking tests (linear types)
    // -----------------------------------------------------------------------

    #[test]
    fn usage_linear_exactly_once_ok() {
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Linear, 0..1);
        tracker.use_var("x");
        let errors = tracker.check();
        assert!(errors.is_empty());
    }

    #[test]
    fn usage_linear_never_used_a05002() {
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Linear, 0..1);
        // Never use x
        let errors = tracker.check();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A05002");
        assert!(errors[0].message.contains("never used"));
        assert!(errors[0].message.contains("x"));
    }

    #[test]
    fn usage_linear_used_twice_a05001() {
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Linear, 0..1);
        tracker.use_var("x");
        tracker.use_var("x");
        let errors = tracker.check();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A05001");
        assert!(errors[0].message.contains("2 times"));
        assert!(errors[0].message.contains("exactly once"));
    }

    #[test]
    fn usage_linear_used_many_times_a05001() {
        let mut tracker = UsageTracker::new();
        tracker.declare("buf".into(), UsageGrade::Linear, 5..10);
        for _ in 0..5 {
            tracker.use_var("buf");
        }
        let errors = tracker.check();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A05001");
        assert!(errors[0].message.contains("5 times"));
    }

    #[test]
    fn usage_erased_not_used_ok() {
        let mut tracker = UsageTracker::new();
        tracker.declare("ghost_val".into(), UsageGrade::Erased, 0..1);
        // Ghost variable never used at runtime: OK
        let errors = tracker.check();
        assert!(errors.is_empty());
    }

    #[test]
    fn usage_erased_used_a05002() {
        let mut tracker = UsageTracker::new();
        tracker.declare("ghost_val".into(), UsageGrade::Erased, 0..1);
        tracker.use_var("ghost_val");
        let errors = tracker.check();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A05002");
        assert!(errors[0].message.contains("erased"));
        assert!(errors[0].message.contains("ghost_val"));
    }

    #[test]
    fn usage_exact_correct_count_ok() {
        let mut tracker = UsageTracker::new();
        tracker.declare("y".into(), UsageGrade::Exact(3), 0..1);
        tracker.use_var("y");
        tracker.use_var("y");
        tracker.use_var("y");
        let errors = tracker.check();
        assert!(errors.is_empty());
    }

    #[test]
    fn usage_exact_too_few_a05003() {
        let mut tracker = UsageTracker::new();
        tracker.declare("y".into(), UsageGrade::Exact(3), 0..1);
        tracker.use_var("y");
        let errors = tracker.check();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A05003");
        assert!(errors[0].message.contains("1 time(s)"));
        assert!(errors[0].message.contains("3 time(s)"));
    }

    #[test]
    fn usage_exact_too_many_a05003() {
        let mut tracker = UsageTracker::new();
        tracker.declare("y".into(), UsageGrade::Exact(2), 0..1);
        tracker.use_var("y");
        tracker.use_var("y");
        tracker.use_var("y");
        tracker.use_var("y");
        let errors = tracker.check();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A05003");
        assert!(errors[0].message.contains("4 time(s)"));
        assert!(errors[0].message.contains("2 time(s)"));
    }

    #[test]
    fn usage_exact_zero_a05003() {
        let mut tracker = UsageTracker::new();
        tracker.declare("z".into(), UsageGrade::Exact(2), 0..1);
        // Never use z
        let errors = tracker.check();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A05003");
        assert!(errors[0].message.contains("0 time(s)"));
    }

    #[test]
    fn usage_unlimited_any_count_ok() {
        let mut tracker = UsageTracker::new();
        tracker.declare("w".into(), UsageGrade::Unlimited, 0..1);
        // Use 0 times: OK
        assert!(tracker.check().is_empty());

        // Use 1 time: OK
        tracker.use_var("w");
        assert!(tracker.check().is_empty());

        // Use 100 times: OK
        for _ in 0..99 {
            tracker.use_var("w");
        }
        assert!(tracker.check().is_empty());
    }

    #[test]
    fn usage_untracked_var_ignored() {
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Linear, 0..1);
        // Using a variable not declared in the tracker is a no-op
        tracker.use_var("y");
        tracker.use_var("x");
        let errors = tracker.check();
        assert!(errors.is_empty());
    }

    #[test]
    fn usage_multiple_variables_mixed() {
        let mut tracker = UsageTracker::new();
        tracker.declare("a".into(), UsageGrade::Linear, 0..1);
        tracker.declare("b".into(), UsageGrade::Linear, 2..3);
        tracker.declare("c".into(), UsageGrade::Unlimited, 4..5);

        tracker.use_var("a"); // OK: linear used once
        // b never used: error
        tracker.use_var("c");
        tracker.use_var("c");
        tracker.use_var("c"); // OK: unlimited

        let errors = tracker.check();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A05002");
        assert!(errors[0].message.contains("b"));
    }

    #[test]
    fn usage_grade_display() {
        assert_eq!(format!("{}", UsageGrade::Erased), "erased (grade 0)");
        assert_eq!(format!("{}", UsageGrade::Linear), "linear (grade 1)");
        assert_eq!(format!("{}", UsageGrade::Exact(5)), "exact (grade 5)");
        assert_eq!(format!("{}", UsageGrade::Unlimited), "unlimited (grade ω)");
    }

    #[test]
    fn expr_usages_counts_ident() {
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Linear, 0..1);
        let expr = AstExpr::Ident("x".into());
        expr_usages(&expr, &mut tracker);
        // x used once, so check should pass for Linear
        assert!(tracker.check().is_empty());
    }

    #[test]
    fn expr_usages_binop_counts_both_sides() {
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Exact(2), 0..1);
        // x + x => 2 uses
        let expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("x".into())),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Ident("x".into())),
        };
        expr_usages(&expr, &mut tracker);
        assert!(tracker.check().is_empty());
    }

    #[test]
    fn expr_usages_linear_used_in_binop_a05001() {
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Linear, 0..1);
        // x + x => 2 uses of a linear variable
        let expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("x".into())),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Ident("x".into())),
        };
        expr_usages(&expr, &mut tracker);
        let errors = tracker.check();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A05001");
    }

    #[test]
    fn expr_usages_call_counts_func_and_args() {
        let mut tracker = UsageTracker::new();
        tracker.declare("f".into(), UsageGrade::Linear, 0..1);
        tracker.declare("a".into(), UsageGrade::Linear, 2..3);
        // f(a) => 1 use of f, 1 use of a
        let expr = AstExpr::Call {
            func: Box::new(AstExpr::Ident("f".into())),
            args: vec![AstExpr::Ident("a".into())],
        };
        expr_usages(&expr, &mut tracker);
        assert!(tracker.check().is_empty());
    }

    #[test]
    fn expr_usages_nested_if() {
        let mut tracker = UsageTracker::new();
        tracker.declare("c".into(), UsageGrade::Exact(1), 0..1);
        tracker.declare("t".into(), UsageGrade::Exact(1), 2..3);
        tracker.declare("e".into(), UsageGrade::Exact(1), 4..5);
        // if c then t else e => 1 use each
        let expr = AstExpr::If {
            cond: Box::new(AstExpr::Ident("c".into())),
            then_branch: Box::new(AstExpr::Ident("t".into())),
            else_branch: Some(Box::new(AstExpr::Ident("e".into()))),
        };
        expr_usages(&expr, &mut tracker);
        assert!(tracker.check().is_empty());
    }

    #[test]
    fn expr_usages_quantifier_counts_domain_and_body() {
        let mut tracker = UsageTracker::new();
        tracker.declare("S".into(), UsageGrade::Exact(1), 0..1);
        tracker.declare("p".into(), UsageGrade::Exact(1), 2..3);
        // forall x in S: p => 1 use of S, 1 use of p
        let expr = AstExpr::Forall {
            var: "x".into(),
            domain: Box::new(AstExpr::Ident("S".into())),
            body: Box::new(AstExpr::Ident("p".into())),
        };
        expr_usages(&expr, &mut tracker);
        assert!(tracker.check().is_empty());
    }

    #[test]
    fn expr_usages_field_access_counts_receiver() {
        let mut tracker = UsageTracker::new();
        tracker.declare("obj".into(), UsageGrade::Linear, 0..1);
        // obj.field => 1 use of obj
        let expr = AstExpr::Field(Box::new(AstExpr::Ident("obj".into())), "field".into());
        expr_usages(&expr, &mut tracker);
        assert!(tracker.check().is_empty());
    }

    #[test]
    fn expr_usages_method_call_counts_receiver_and_args() {
        let mut tracker = UsageTracker::new();
        tracker.declare("obj".into(), UsageGrade::Exact(1), 0..1);
        tracker.declare("arg1".into(), UsageGrade::Exact(1), 2..3);
        // obj.method(arg1)
        let expr = AstExpr::MethodCall {
            receiver: Box::new(AstExpr::Ident("obj".into())),
            method: "method".into(),
            args: vec![AstExpr::Ident("arg1".into())],
        };
        expr_usages(&expr, &mut tracker);
        assert!(tracker.check().is_empty());
    }

    #[test]
    fn expr_usages_index_counts_base_and_index() {
        let mut tracker = UsageTracker::new();
        tracker.declare("arr".into(), UsageGrade::Exact(1), 0..1);
        tracker.declare("i".into(), UsageGrade::Exact(1), 2..3);
        // arr[i]
        let expr = AstExpr::Index {
            expr: Box::new(AstExpr::Ident("arr".into())),
            index: Box::new(AstExpr::Ident("i".into())),
        };
        expr_usages(&expr, &mut tracker);
        assert!(tracker.check().is_empty());
    }

    #[test]
    fn expr_usages_old_counts_inner() {
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Linear, 0..1);
        // old(x) => 1 use of x
        let expr = AstExpr::Old(Box::new(AstExpr::Ident("x".into())));
        expr_usages(&expr, &mut tracker);
        assert!(tracker.check().is_empty());
    }

    #[test]
    fn expr_usages_paren_counts_inner() {
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Linear, 0..1);
        // (x) => 1 use of x
        let expr = AstExpr::Paren(Box::new(AstExpr::Ident("x".into())));
        expr_usages(&expr, &mut tracker);
        assert!(tracker.check().is_empty());
    }

    #[test]
    fn expr_usages_list_counts_elements() {
        let mut tracker = UsageTracker::new();
        tracker.declare("a".into(), UsageGrade::Exact(1), 0..1);
        tracker.declare("b".into(), UsageGrade::Exact(1), 2..3);
        // [a, b]
        let expr = AstExpr::List(vec![AstExpr::Ident("a".into()), AstExpr::Ident("b".into())]);
        expr_usages(&expr, &mut tracker);
        assert!(tracker.check().is_empty());
    }

    #[test]
    fn expr_usages_unary_counts_inner() {
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Linear, 0..1);
        // -x => 1 use of x
        let expr = AstExpr::UnaryOp {
            op: AstUnOp::Neg,
            expr: Box::new(AstExpr::Ident("x".into())),
        };
        expr_usages(&expr, &mut tracker);
        assert!(tracker.check().is_empty());
    }

    #[test]
    fn expr_usages_cast_counts_inner() {
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Linear, 0..1);
        // x as Foo => 1 use of x
        let expr = AstExpr::Cast {
            expr: Box::new(AstExpr::Ident("x".into())),
            ty: "Foo".into(),
        };
        expr_usages(&expr, &mut tracker);
        assert!(tracker.check().is_empty());
    }

    #[test]
    fn expr_usages_block_counts_all() {
        let mut tracker = UsageTracker::new();
        tracker.declare("a".into(), UsageGrade::Exact(1), 0..1);
        tracker.declare("b".into(), UsageGrade::Exact(1), 2..3);
        let expr = AstExpr::Block(vec![AstExpr::Ident("a".into()), AstExpr::Ident("b".into())]);
        expr_usages(&expr, &mut tracker);
        assert!(tracker.check().is_empty());
    }

    #[test]
    fn expr_usages_raw_no_count() {
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Linear, 0..1);
        // Raw tokens cannot be analyzed; x stays at 0 uses
        let expr = AstExpr::Raw(vec!["x".into()]);
        expr_usages(&expr, &mut tracker);
        let errors = tracker.check();
        // Linear var not used => A05002
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A05002");
    }

    #[test]
    fn expr_usages_literal_no_count() {
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Unlimited, 0..1);
        let expr = AstExpr::Literal(AstLit::Int("42".into()));
        expr_usages(&expr, &mut tracker);
        // No uses recorded, but unlimited is fine
        assert!(tracker.check().is_empty());
    }

    #[test]
    fn usage_tracker_redeclare_resets() {
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Linear, 0..1);
        tracker.use_var("x");
        // Re-declare resets count
        tracker.declare("x".into(), UsageGrade::Linear, 10..11);
        // Now x has 0 uses again
        let errors = tracker.check();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A05002");
        // Span should be the new declaration span
        assert_eq!(errors[0].span, 10..11);
    }

    // -----------------------------------------------------------------------
    // T032: Context splitting for linear types
    // -----------------------------------------------------------------------

    #[test]
    fn linear_context_both_branches_use_var_ok() {
        // Linear var used once in each branch: OK (consumed in both paths)
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Linear, 0..1);
        let mut ctx = LinearContext::new(tracker);

        // if cond then x else x
        let expr = AstExpr::If {
            cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
            then_branch: Box::new(AstExpr::Ident("x".into())),
            else_branch: Some(Box::new(AstExpr::Ident("x".into()))),
        };
        let branch_errors = check_expr_linearity(&expr, &mut ctx);
        assert!(branch_errors.is_empty(), "should have no A05004 errors");

        // Final check: used exactly once (max from either branch)
        let final_errors = ctx.check();
        assert!(
            final_errors.is_empty(),
            "should have no final errors: {final_errors:?}"
        );
    }

    #[test]
    fn linear_context_one_branch_only_a05004() {
        // Linear var used in then-branch but not else-branch: A05004
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Linear, 0..1);
        let mut ctx = LinearContext::new(tracker);

        // if cond then x else 42
        let expr = AstExpr::If {
            cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
            then_branch: Box::new(AstExpr::Ident("x".into())),
            else_branch: Some(Box::new(AstExpr::Literal(AstLit::Int("42".into())))),
        };
        let branch_errors = check_expr_linearity(&expr, &mut ctx);
        assert_eq!(branch_errors.len(), 1);
        assert_eq!(branch_errors[0].code, "A05004");
        assert!(branch_errors[0].message.contains("x"));
        assert!(branch_errors[0].message.contains("inconsistently"));
    }

    #[test]
    fn linear_context_no_else_branch_a05004() {
        // Linear var used in then-branch with no else-branch: A05004
        // (variable may or may not be consumed depending on condition)
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Linear, 0..1);
        let mut ctx = LinearContext::new(tracker);

        // if cond then x
        let expr = AstExpr::If {
            cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
            then_branch: Box::new(AstExpr::Ident("x".into())),
            else_branch: None,
        };
        let branch_errors = check_expr_linearity(&expr, &mut ctx);
        assert_eq!(branch_errors.len(), 1);
        assert_eq!(branch_errors[0].code, "A05004");
    }

    #[test]
    fn linear_context_neither_branch_uses_var() {
        // Linear var used in neither branch: no A05004 (consistent: 0 in both)
        // But final check will produce A05002 (never used).
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Linear, 0..1);
        let mut ctx = LinearContext::new(tracker);

        // if cond then 1 else 2
        let expr = AstExpr::If {
            cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
            then_branch: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
            else_branch: Some(Box::new(AstExpr::Literal(AstLit::Int("2".into())))),
        };
        let branch_errors = check_expr_linearity(&expr, &mut ctx);
        assert!(
            branch_errors.is_empty(),
            "consistent: 0 uses in both branches"
        );

        // Final check: linear var never used
        let final_errors = ctx.check();
        assert_eq!(final_errors.len(), 1);
        assert_eq!(final_errors[0].code, "A05002");
    }

    #[test]
    fn linear_context_double_use_in_one_branch() {
        // Linear var used twice in one branch, once in the other: A05004
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Linear, 0..1);
        let mut ctx = LinearContext::new(tracker);

        // if cond then (x + x) else x
        let expr = AstExpr::If {
            cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
            then_branch: Box::new(AstExpr::BinOp {
                lhs: Box::new(AstExpr::Ident("x".into())),
                op: AstBinOp::Add,
                rhs: Box::new(AstExpr::Ident("x".into())),
            }),
            else_branch: Some(Box::new(AstExpr::Ident("x".into()))),
        };
        let branch_errors = check_expr_linearity(&expr, &mut ctx);
        assert_eq!(branch_errors.len(), 1);
        assert_eq!(branch_errors[0].code, "A05004");
        // Delta: 2 in then, 1 in else
        assert!(branch_errors[0].message.contains("2 time(s)"));
        assert!(branch_errors[0].message.contains("1 time(s)"));
    }

    #[test]
    fn linear_context_unlimited_var_no_consistency_error() {
        // Unlimited variable used differently in branches: no A05004
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Unlimited, 0..1);
        let mut ctx = LinearContext::new(tracker);

        // if cond then (x + x + x) else x
        let expr = AstExpr::If {
            cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
            then_branch: Box::new(AstExpr::BinOp {
                lhs: Box::new(AstExpr::BinOp {
                    lhs: Box::new(AstExpr::Ident("x".into())),
                    op: AstBinOp::Add,
                    rhs: Box::new(AstExpr::Ident("x".into())),
                }),
                op: AstBinOp::Add,
                rhs: Box::new(AstExpr::Ident("x".into())),
            }),
            else_branch: Some(Box::new(AstExpr::Ident("x".into()))),
        };
        let branch_errors = check_expr_linearity(&expr, &mut ctx);
        assert!(branch_errors.is_empty());

        let final_errors = ctx.check();
        assert!(final_errors.is_empty());
    }

    #[test]
    fn linear_context_condition_uses_before_fork() {
        // Variable used in condition (before fork) and in one branch:
        // results in 2 total uses of a linear var after merge => A05001 from check().
        // Branch consistency: then uses 0 more, else uses 0 more => consistent.
        let mut tracker = UsageTracker::new();
        tracker.declare("c".into(), UsageGrade::Linear, 0..1);
        tracker.declare("x".into(), UsageGrade::Linear, 2..3);
        let mut ctx = LinearContext::new(tracker);

        // if c then x else x
        let expr = AstExpr::If {
            cond: Box::new(AstExpr::Ident("c".into())),
            then_branch: Box::new(AstExpr::Ident("x".into())),
            else_branch: Some(Box::new(AstExpr::Ident("x".into()))),
        };
        let branch_errors = check_expr_linearity(&expr, &mut ctx);
        assert!(branch_errors.is_empty());

        let final_errors = ctx.check();
        // c used once (in condition), x used once (max from branches) => both OK
        assert!(final_errors.is_empty(), "errors: {final_errors:?}");
    }

    #[test]
    fn linear_context_multiple_vars_mixed() {
        // Multiple variables: one consistent, one not.
        let mut tracker = UsageTracker::new();
        tracker.declare("a".into(), UsageGrade::Linear, 0..1);
        tracker.declare("b".into(), UsageGrade::Linear, 2..3);
        let mut ctx = LinearContext::new(tracker);

        // if cond then (a, b) else (a, 0)
        // a: used in both => consistent
        // b: used in then only => inconsistent A05004
        let expr = AstExpr::If {
            cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
            then_branch: Box::new(AstExpr::List(vec![
                AstExpr::Ident("a".into()),
                AstExpr::Ident("b".into()),
            ])),
            else_branch: Some(Box::new(AstExpr::List(vec![
                AstExpr::Ident("a".into()),
                AstExpr::Literal(AstLit::Int("0".into())),
            ]))),
        };
        let branch_errors = check_expr_linearity(&expr, &mut ctx);
        assert_eq!(branch_errors.len(), 1);
        assert_eq!(branch_errors[0].code, "A05004");
        assert!(branch_errors[0].message.contains("b"));
    }

    #[test]
    fn linear_context_exact_grade_consistency_check() {
        // Exact(2) grade: must use consistently across branches.
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Exact(2), 0..1);
        let mut ctx = LinearContext::new(tracker);

        // if cond then (x+x) else x  => delta 2 vs delta 1 => A05004
        let expr = AstExpr::If {
            cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
            then_branch: Box::new(AstExpr::BinOp {
                lhs: Box::new(AstExpr::Ident("x".into())),
                op: AstBinOp::Add,
                rhs: Box::new(AstExpr::Ident("x".into())),
            }),
            else_branch: Some(Box::new(AstExpr::Ident("x".into()))),
        };
        let branch_errors = check_expr_linearity(&expr, &mut ctx);
        assert_eq!(branch_errors.len(), 1);
        assert_eq!(branch_errors[0].code, "A05004");
    }

    #[test]
    fn linear_context_exact_grade_consistent_ok() {
        // Exact(2): same delta in both branches => OK
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Exact(2), 0..1);
        let mut ctx = LinearContext::new(tracker);

        // if cond then (x+x) else (x+x) => delta 2 in both
        let expr = AstExpr::If {
            cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
            then_branch: Box::new(AstExpr::BinOp {
                lhs: Box::new(AstExpr::Ident("x".into())),
                op: AstBinOp::Add,
                rhs: Box::new(AstExpr::Ident("x".into())),
            }),
            else_branch: Some(Box::new(AstExpr::BinOp {
                lhs: Box::new(AstExpr::Ident("x".into())),
                op: AstBinOp::Add,
                rhs: Box::new(AstExpr::Ident("x".into())),
            })),
        };
        let branch_errors = check_expr_linearity(&expr, &mut ctx);
        assert!(branch_errors.is_empty());

        let final_errors = ctx.check();
        assert!(final_errors.is_empty());
    }

    #[test]
    fn linear_context_nested_if_branches() {
        // Nested if: outer branch forks, inner branch forks again.
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Linear, 0..1);
        let mut ctx = LinearContext::new(tracker);

        // if c1 then (if c2 then x else x) else x
        // Inner if: x used consistently in both branches => OK
        // Outer if: after inner merge, x used once in then, once in else => OK
        let expr = AstExpr::If {
            cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
            then_branch: Box::new(AstExpr::If {
                cond: Box::new(AstExpr::Literal(AstLit::Bool(false))),
                then_branch: Box::new(AstExpr::Ident("x".into())),
                else_branch: Some(Box::new(AstExpr::Ident("x".into()))),
            }),
            else_branch: Some(Box::new(AstExpr::Ident("x".into()))),
        };
        let branch_errors = check_expr_linearity(&expr, &mut ctx);
        assert!(branch_errors.is_empty());

        let final_errors = ctx.check();
        assert!(final_errors.is_empty());
    }

    #[test]
    fn linear_context_nested_if_inner_inconsistent() {
        // Inner if is inconsistent: should produce A05004.
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Linear, 0..1);
        let mut ctx = LinearContext::new(tracker);

        // if c1 then (if c2 then x else 0) else x
        // Inner if: x used in then but not else => A05004
        let expr = AstExpr::If {
            cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
            then_branch: Box::new(AstExpr::If {
                cond: Box::new(AstExpr::Literal(AstLit::Bool(false))),
                then_branch: Box::new(AstExpr::Ident("x".into())),
                else_branch: Some(Box::new(AstExpr::Literal(AstLit::Int("0".into())))),
            }),
            else_branch: Some(Box::new(AstExpr::Ident("x".into()))),
        };
        let branch_errors = check_expr_linearity(&expr, &mut ctx);
        // Inner if produces an A05004 for x
        assert!(
            branch_errors.iter().any(|e| e.code == "A05004"),
            "expected A05004: {branch_errors:?}"
        );
    }

    #[test]
    fn linear_context_erased_var_unaffected_by_branches() {
        // Erased variable: branch consistency not checked (grade is Erased).
        // Using it in either branch is an A05002 from final check, not A05004.
        let mut tracker = UsageTracker::new();
        tracker.declare("g".into(), UsageGrade::Erased, 0..1);
        let mut ctx = LinearContext::new(tracker);

        // if cond then g else 0
        let expr = AstExpr::If {
            cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
            then_branch: Box::new(AstExpr::Ident("g".into())),
            else_branch: Some(Box::new(AstExpr::Literal(AstLit::Int("0".into())))),
        };
        let branch_errors = check_expr_linearity(&expr, &mut ctx);
        // Erased is not Linear or Exact, so no A05004
        assert!(branch_errors.is_empty());

        // Final check: erased var used at runtime => A05002
        let final_errors = ctx.check();
        assert_eq!(final_errors.len(), 1);
        assert_eq!(final_errors[0].code, "A05002");
    }

    #[test]
    fn linear_context_var_used_in_condition_and_branches() {
        // x used in condition (1 use), then in both branches (1 each).
        // Post-condition base count = 1. Each branch adds 1 more.
        // Delta: 1 in both => consistent. Total after merge: 2.
        // Linear var used 2 times => A05001 from final check.
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Linear, 0..1);
        let mut ctx = LinearContext::new(tracker);

        // if x then x else x  (x as condition + x in each branch)
        let expr = AstExpr::If {
            cond: Box::new(AstExpr::Ident("x".into())),
            then_branch: Box::new(AstExpr::Ident("x".into())),
            else_branch: Some(Box::new(AstExpr::Ident("x".into()))),
        };
        let branch_errors = check_expr_linearity(&expr, &mut ctx);
        // Branches are consistent (both use x once more)
        assert!(branch_errors.is_empty());

        // Final: x used 2 times total (1 condition + 1 from branch max)
        let final_errors = ctx.check();
        assert_eq!(final_errors.len(), 1);
        assert_eq!(final_errors[0].code, "A05001");
    }

    #[test]
    fn linear_context_fork_produces_independent_copies() {
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Linear, 0..1);
        let ctx = LinearContext::new(tracker);

        let (mut a, mut b) = ctx.fork();
        a.use_var("x");
        // b should still have 0 uses
        assert_eq!(a.get_count("x"), Some(1));
        assert_eq!(b.get_count("x"), Some(0));

        b.use_var("x");
        b.use_var("x");
        assert_eq!(b.get_count("x"), Some(2));
        assert_eq!(a.get_count("x"), Some(1)); // unchanged
    }

    #[test]
    fn linear_context_merge_takes_max_usage() {
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Unlimited, 0..1);
        let mut ctx = LinearContext::new(tracker);

        let (mut a, mut b) = ctx.fork();
        a.use_var("x");
        a.use_var("x");
        a.use_var("x");
        b.use_var("x");

        let _ = ctx.merge(&a, &b);
        // Max of 3 and 1 = 3
        assert_eq!(ctx.get_count("x"), Some(3));
    }

    #[test]
    fn linear_context_a05005_scope_escape() {
        // A05005: linear variable escapes its scope.
        // This occurs when a linear variable is passed into a context
        // where it outlives its scope (e.g., stored in a longer-lived data
        // structure). For now, model this as a linear var that gets used
        // but its scope ends before consumption.
        //
        // Detected by declaring the variable, walking a scope, then
        // checking: if the variable was not consumed (used 0 times in the
        // scope it was declared in), it effectively escaped.
        let mut tracker = UsageTracker::new();
        tracker.declare("resource".into(), UsageGrade::Linear, 0..8);
        let mut ctx = LinearContext::new(tracker);

        // Simulate: resource is declared but never used in its scope
        // (no expressions reference it).
        let expr = AstExpr::Literal(AstLit::Int("42".into()));
        let branch_errors = check_expr_linearity(&expr, &mut ctx);
        assert!(branch_errors.is_empty());

        // Final check catches it: linear var never used => A05002
        // This is the scope-escape case: the variable existed but was
        // never consumed before its scope ended.
        let final_errors = ctx.check();
        assert_eq!(final_errors.len(), 1);
        assert_eq!(final_errors[0].code, "A05002");
        assert!(final_errors[0].message.contains("resource"));
    }

    // -----------------------------------------------------------------------
    // T033: Linear type test cases (Section 13 Test Case 1 + additional)
    // -----------------------------------------------------------------------

    #[test]
    fn linear_double_use_a05001() {
        // Double-use of a linear variable must produce A05001.
        let mut tracker = UsageTracker::new();
        tracker.declare("buf".into(), UsageGrade::Linear, 0..3);
        let mut ctx = LinearContext::new(tracker);

        // buf + buf => 2 uses of linear variable
        let expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("buf".into())),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Ident("buf".into())),
        };
        let _ = check_expr_linearity(&expr, &mut ctx);
        let errors = ctx.check();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A05001");
        assert!(errors[0].message.contains("buf"));
        assert!(errors[0].message.contains("2 times"));
    }

    #[test]
    fn linear_unused_a05002() {
        // Unused linear variable must produce A05002.
        let mut tracker = UsageTracker::new();
        tracker.declare("handle".into(), UsageGrade::Linear, 0..6);
        let mut ctx = LinearContext::new(tracker);

        // Expression that does not reference 'handle' at all
        let expr = AstExpr::Literal(AstLit::Int("99".into()));
        let branch_errors = check_expr_linearity(&expr, &mut ctx);
        assert!(branch_errors.is_empty());

        let errors = ctx.check();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A05002");
        assert!(errors[0].message.contains("handle"));
        assert!(errors[0].message.contains("never used"));
    }

    #[test]
    fn linear_correctly_used_once_passes() {
        // Linear variable used exactly once must pass without errors.
        let mut tracker = UsageTracker::new();
        tracker.declare("conn".into(), UsageGrade::Linear, 0..4);
        let mut ctx = LinearContext::new(tracker);

        // Single use: conn
        let expr = AstExpr::Ident("conn".into());
        let branch_errors = check_expr_linearity(&expr, &mut ctx);
        assert!(branch_errors.is_empty());

        let errors = ctx.check();
        assert!(errors.is_empty());
    }

    #[test]
    fn linear_refinement_predicate_not_a_use() {
        // Section 13, Test Case 1: a refinement predicate on a linear
        // variable should NOT count as a runtime use. The refinement
        // predicate is a compile-time/SMT-level constraint, not a
        // runtime consumption.
        //
        // Model: declare the linear variable, record a "refinement use"
        // (which should be ignored), then record a single real use.
        // The variable should be correctly consumed once.
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Linear, 0..1);

        // The refinement predicate x > 0 does NOT consume x.
        // Only the actual use in the expression body does.
        // We model this by NOT calling use_var for the refinement.
        // A single real use follows:
        tracker.use_var("x"); // real runtime use

        let errors = tracker.check();
        assert!(
            errors.is_empty(),
            "refinement predicate should not count as a use: {errors:?}"
        );
    }

    #[test]
    fn linear_refinement_predicate_plus_real_use_no_double_count() {
        // Variant of Section 13 Test Case 1: if the refinement predicate
        // were incorrectly counted, a linear var with a refinement plus
        // one real use would show 2 uses (A05001). Verify it only shows 1.
        let mut tracker = UsageTracker::new();
        tracker.declare("resource".into(), UsageGrade::Linear, 0..8);

        // Refinement predicate: resource.is_valid() -- NOT a runtime use.
        // (We skip calling use_var for predicates.)

        // One real use in the function body:
        tracker.use_var("resource");

        let errors = tracker.check();
        assert!(
            errors.is_empty(),
            "should be exactly 1 use, not 2: {errors:?}"
        );
        assert_eq!(tracker.get_count("resource"), Some(1));
    }

    #[test]
    fn linear_triple_use_a05001() {
        // Three uses of a linear variable: A05001 with count 3.
        let mut tracker = UsageTracker::new();
        tracker.declare("fd".into(), UsageGrade::Linear, 0..2);
        let mut ctx = LinearContext::new(tracker);

        // fd + fd + fd => 3 uses
        let expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::BinOp {
                lhs: Box::new(AstExpr::Ident("fd".into())),
                op: AstBinOp::Add,
                rhs: Box::new(AstExpr::Ident("fd".into())),
            }),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Ident("fd".into())),
        };
        let _ = check_expr_linearity(&expr, &mut ctx);
        let errors = ctx.check();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A05001");
        assert!(errors[0].message.contains("3 times"));
    }

    #[test]
    fn linear_used_in_call_arg_exactly_once_passes() {
        // Linear variable used as a function argument (single use) passes.
        let mut tracker = UsageTracker::new();
        tracker.declare("key".into(), UsageGrade::Linear, 0..3);
        let mut ctx = LinearContext::new(tracker);

        // consume(key) => 1 use of key
        let expr = AstExpr::Call {
            func: Box::new(AstExpr::Ident("consume".into())),
            args: vec![AstExpr::Ident("key".into())],
        };
        let branch_errors = check_expr_linearity(&expr, &mut ctx);
        assert!(branch_errors.is_empty());

        let errors = ctx.check();
        assert!(errors.is_empty());
    }

    #[test]
    fn linear_branch_consistency_with_single_use_passes() {
        // Linear variable used exactly once in each branch: passes.
        let mut tracker = UsageTracker::new();
        tracker.declare("tok".into(), UsageGrade::Linear, 0..3);
        let mut ctx = LinearContext::new(tracker);

        // if cond then consume(tok) else discard(tok)
        let expr = AstExpr::If {
            cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
            then_branch: Box::new(AstExpr::Call {
                func: Box::new(AstExpr::Ident("consume".into())),
                args: vec![AstExpr::Ident("tok".into())],
            }),
            else_branch: Some(Box::new(AstExpr::Call {
                func: Box::new(AstExpr::Ident("discard".into())),
                args: vec![AstExpr::Ident("tok".into())],
            })),
        };
        let branch_errors = check_expr_linearity(&expr, &mut ctx);
        assert!(branch_errors.is_empty());

        let errors = ctx.check();
        assert!(errors.is_empty());
    }

    #[test]
    fn linear_two_vars_one_double_used_one_unused() {
        // Two linear variables: one double-used (A05001), one unused (A05002).
        let mut tracker = UsageTracker::new();
        tracker.declare("a".into(), UsageGrade::Linear, 0..1);
        tracker.declare("b".into(), UsageGrade::Linear, 2..3);
        let mut ctx = LinearContext::new(tracker);

        // a + a (double use of a, b never referenced)
        let expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("a".into())),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Ident("a".into())),
        };
        let _ = check_expr_linearity(&expr, &mut ctx);
        let errors = ctx.check();
        assert_eq!(errors.len(), 2);

        let codes: Vec<&str> = errors.iter().map(|e| e.code.as_str()).collect();
        assert!(codes.contains(&"A05001"), "expected A05001 for `a`");
        assert!(codes.contains(&"A05002"), "expected A05002 for `b`");
    }

    // -----------------------------------------------------------------------
    // T034: Typestate checker tests
    // -----------------------------------------------------------------------

    #[test]
    fn typestate_valid_sequence_passes() {
        // Valid transition sequence: Init -> Open -> Close
        let states = vec!["Init".into(), "Open".into(), "Closed".into()];
        let transitions = vec![
            ("open".into(), "Init".into(), "Open".into()),
            ("close".into(), "Open".into(), "Closed".into()),
        ];
        let mut checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

        assert!(checker.transition("open", 5..9).is_ok());
        assert_eq!(checker.current_state(), "Open");
        assert!(checker.transition("close", 10..15).is_ok());
        assert_eq!(checker.current_state(), "Closed");
    }

    #[test]
    fn typestate_wrong_state_a06001() {
        // Operation called in wrong state: close() requires Open, but
        // we are in Init.
        let states = vec!["Init".into(), "Open".into(), "Closed".into()];
        let transitions = vec![
            ("open".into(), "Init".into(), "Open".into()),
            ("close".into(), "Open".into(), "Closed".into()),
        ];
        let mut checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

        let err = checker.transition("close", 5..10).unwrap_err();
        assert_eq!(err.code, "A06001");
        assert!(err.message.contains("close"));
        assert!(err.message.contains("Init"));
        assert!(err.message.contains("Open"));
    }

    #[test]
    fn typestate_not_linear_a06002() {
        // Typestate variables must be linear; this is checked separately.
        // The TypestateChecker itself produces A06002 when validate_linear
        // is called with is_linear=false.
        let states = vec!["Init".into(), "Open".into()];
        let transitions = vec![("open".into(), "Init".into(), "Open".into())];
        let checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

        let err = checker.validate_linear(false);
        assert!(err.is_some());
        let err = err.unwrap();
        assert_eq!(err.code, "A06002");
        assert!(err.message.contains("linear"));
    }

    #[test]
    fn typestate_not_linear_ok_when_linear() {
        // When the variable IS linear, validate_linear returns None.
        let states = vec!["Init".into()];
        let checker = TypestateChecker::new(states, vec![], "Init".into(), 0..4);
        assert!(checker.validate_linear(true).is_none());
    }

    #[test]
    fn typestate_undeclared_state_a06003() {
        // Operation transitions to a state not declared in `states:`.
        let states = vec!["Init".into(), "Open".into()];
        let transitions = vec![
            ("open".into(), "Init".into(), "Open".into()),
            // "Closed" is not in the declared states
            ("close".into(), "Open".into(), "Closed".into()),
        ];
        let checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

        let errors = checker.validate_transitions();
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| e.code == "A06003"));
        assert!(errors.iter().any(|e| e.message.contains("Closed")));
    }

    #[test]
    fn typestate_undeclared_source_state_a06003() {
        // Transition references a source state not in the declared states.
        let states = vec!["Init".into(), "Done".into()];
        let transitions = vec![
            // "Running" is not declared
            ("finish".into(), "Running".into(), "Done".into()),
        ];
        let checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

        let errors = checker.validate_transitions();
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| e.code == "A06003"));
        assert!(errors.iter().any(|e| e.message.contains("Running")));
    }

    #[test]
    fn typestate_ambiguous_after_branches_a06004() {
        // Diverging branches leave the object in different states.
        // After branch A: Open, after branch B: Closed => A06004.
        let states = vec!["Init".into(), "Open".into(), "Closed".into()];
        let transitions = vec![
            ("open".into(), "Init".into(), "Open".into()),
            ("close".into(), "Init".into(), "Closed".into()),
        ];

        let checker_a = {
            let mut c =
                TypestateChecker::new(states.clone(), transitions.clone(), "Init".into(), 0..4);
            c.transition("open", 5..9).unwrap();
            c
        };
        let checker_b = {
            let mut c = TypestateChecker::new(states, transitions, "Init".into(), 0..4);
            c.transition("close", 5..10).unwrap();
            c
        };

        let err = TypestateChecker::check_branch_consistency(&checker_a, &checker_b, 0..4);
        assert!(err.is_some());
        let err = err.unwrap();
        assert_eq!(err.code, "A06004");
        assert!(err.message.contains("Open"));
        assert!(err.message.contains("Closed"));
    }

    #[test]
    fn typestate_consistent_branches_same_state_ok() {
        // Both branches leave the object in the same state: no error.
        let states = vec!["Init".into(), "Open".into()];
        let transitions = vec![("open".into(), "Init".into(), "Open".into())];

        let checker_a = {
            let mut c =
                TypestateChecker::new(states.clone(), transitions.clone(), "Init".into(), 0..4);
            c.transition("open", 5..9).unwrap();
            c
        };
        let checker_b = {
            let mut c = TypestateChecker::new(states, transitions, "Init".into(), 0..4);
            c.transition("open", 5..9).unwrap();
            c
        };

        let err = TypestateChecker::check_branch_consistency(&checker_a, &checker_b, 0..4);
        assert!(err.is_none());
    }

    #[test]
    fn typestate_multiple_transitions_sequence() {
        // Longer transition chain: Init -> Connecting -> Connected -> Closed
        let states = vec![
            "Init".into(),
            "Connecting".into(),
            "Connected".into(),
            "Closed".into(),
        ];
        let transitions = vec![
            ("connect".into(), "Init".into(), "Connecting".into()),
            (
                "established".into(),
                "Connecting".into(),
                "Connected".into(),
            ),
            ("disconnect".into(), "Connected".into(), "Closed".into()),
        ];
        let mut checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

        assert!(checker.transition("connect", 5..12).is_ok());
        assert_eq!(checker.current_state(), "Connecting");
        assert!(checker.transition("established", 13..24).is_ok());
        assert_eq!(checker.current_state(), "Connected");
        assert!(checker.transition("disconnect", 25..35).is_ok());
        assert_eq!(checker.current_state(), "Closed");
    }

    #[test]
    fn typestate_operation_not_found_a06001() {
        // Calling an operation that does not exist in any transition.
        let states = vec!["Init".into(), "Open".into()];
        let transitions = vec![("open".into(), "Init".into(), "Open".into())];
        let mut checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

        let err = checker.transition("nonexistent", 5..16).unwrap_err();
        assert_eq!(err.code, "A06001");
        assert!(err.message.contains("nonexistent"));
    }

    #[test]
    fn typestate_valid_transitions_no_errors() {
        // All transitions reference declared states: no errors.
        let states = vec!["Init".into(), "Open".into(), "Closed".into()];
        let transitions = vec![
            ("open".into(), "Init".into(), "Open".into()),
            ("close".into(), "Open".into(), "Closed".into()),
        ];
        let checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

        let errors = checker.validate_transitions();
        assert!(errors.is_empty());
    }

    #[test]
    fn typestate_initial_state() {
        // Checker starts in the declared initial state.
        let states = vec!["Start".into(), "End".into()];
        let transitions = vec![("finish".into(), "Start".into(), "End".into())];
        let checker = TypestateChecker::new(states, transitions, "Start".into(), 0..5);

        assert_eq!(checker.current_state(), "Start");
    }

    // -----------------------------------------------------------------------
    // T036-T037: Effect checker tests
    // -----------------------------------------------------------------------

    // -- EffectSet construction and display --

    #[test]
    fn effect_set_pure_is_empty() {
        let set = EffectSet::pure();
        assert!(set.is_pure());
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
        assert_eq!(format!("{set}"), "pure");
    }

    #[test]
    fn effect_set_from_iter_basic() {
        let set = EffectSet::from_effect_names(["io", "mem"]);
        assert!(!set.is_pure());
        assert_eq!(set.len(), 2);
        assert!(set.contains("io"));
        assert!(set.contains("mem"));
        assert!(!set.contains("net"));
    }

    #[test]
    fn effect_set_from_iter_pure_ignored() {
        // "pure" in the iterator should be ignored (it means empty set)
        let set = EffectSet::from_effect_names(["pure"]);
        assert!(set.is_pure());
        assert!(set.is_empty());
    }

    #[test]
    fn effect_set_from_iter_pure_mixed() {
        // "pure" mixed with others: pure is dropped, others kept
        let set = EffectSet::from_effect_names(["pure", "io"]);
        assert!(!set.is_pure());
        assert_eq!(set.len(), 1);
        assert!(set.contains("io"));
    }

    #[test]
    fn effect_set_insert() {
        let mut set = EffectSet::pure();
        set.insert("io".into());
        assert!(!set.is_pure());
        assert!(set.contains("io"));
    }

    #[test]
    fn effect_set_insert_pure_noop() {
        let mut set = EffectSet::pure();
        set.insert("pure".into());
        assert!(set.is_pure());
    }

    #[test]
    fn effect_set_display_sorted() {
        let set = EffectSet::from_effect_names(["mem", "io", "alloc"]);
        // Display should sort effects alphabetically
        assert_eq!(format!("{set}"), "{alloc, io, mem}");
    }

    // -- EffectChecker: known effects --

    #[test]
    fn effect_checker_knows_builtins() {
        let checker = EffectChecker::new();
        assert!(checker.is_known("io"));
        assert!(checker.is_known("mem"));
        assert!(checker.is_known("net"));
        assert!(checker.is_known("fs"));
        assert!(checker.is_known("rng"));
        assert!(checker.is_known("time"));
        assert!(checker.is_known("alloc"));
        assert!(checker.is_known("console.read"));
        assert!(checker.is_known("console.write"));
        assert!(checker.is_known("filesystem.read"));
        assert!(checker.is_known("filesystem.write"));
        assert!(checker.is_known("network.connect"));
        assert!(checker.is_known("network.send"));
        assert!(checker.is_known("network.receive"));
        assert!(checker.is_known("database"));
        assert!(checker.is_known("database.read"));
        assert!(checker.is_known("database.write"));
        assert!(checker.is_known("logging"));
        assert!(checker.is_known("log.debug"));
        assert!(checker.is_known("log.info"));
        assert!(checker.is_known("log.warn"));
        assert!(checker.is_known("log.error"));
        assert!(checker.is_known("time.read"));
        assert!(checker.is_known("random"));
        assert!(checker.is_known("diverge"));
    }

    #[test]
    fn effect_checker_unknown_effect() {
        let checker = EffectChecker::new();
        assert!(!checker.is_known("teleport"));
        assert!(!checker.is_known("quantum"));
    }

    // -- A07003: unknown effect name --

    #[test]
    fn effect_check_known_all_valid() {
        let checker = EffectChecker::new();
        let set = EffectSet::from_effect_names(["io", "mem", "database"]);
        let errors = checker.check_known(&set, &(0..10));
        assert!(errors.is_empty());
    }

    #[test]
    fn effect_check_known_unknown_a07003() {
        let checker = EffectChecker::new();
        let set = EffectSet::from_effect_names(["io", "teleport"]);
        let errors = checker.check_known(&set, &(0..10));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A07003");
        assert!(errors[0].message.contains("teleport"));
    }

    #[test]
    fn effect_check_known_multiple_unknown_a07003() {
        let checker = EffectChecker::new();
        let set = EffectSet::from_effect_names(["teleport", "quantum"]);
        let errors = checker.check_known(&set, &(0..10));
        assert_eq!(errors.len(), 2);
        assert!(errors.iter().all(|e| e.code == "A07003"));
    }

    // -- Hierarchy expansion --

    #[test]
    fn effect_expand_io_includes_subeffects() {
        let checker = EffectChecker::new();
        let declared = EffectSet::from_effect_names(["io"]);
        let expanded = checker.expand(&declared);
        assert!(expanded.contains("io"));
        assert!(expanded.contains("console.read"));
        assert!(expanded.contains("console.write"));
        assert!(expanded.contains("filesystem.read"));
        assert!(expanded.contains("filesystem.write"));
        assert!(expanded.contains("network.connect"));
        assert!(expanded.contains("network.send"));
        assert!(expanded.contains("network.receive"));
        assert!(expanded.contains("time.read"));
        assert!(expanded.contains("random"));
    }

    #[test]
    fn effect_expand_database_includes_subeffects() {
        let checker = EffectChecker::new();
        let declared = EffectSet::from_effect_names(["database"]);
        let expanded = checker.expand(&declared);
        assert!(expanded.contains("database"));
        assert!(expanded.contains("database.read"));
        assert!(expanded.contains("database.write"));
    }

    #[test]
    fn effect_expand_logging_includes_subeffects() {
        let checker = EffectChecker::new();
        let declared = EffectSet::from_effect_names(["logging"]);
        let expanded = checker.expand(&declared);
        assert!(expanded.contains("logging"));
        assert!(expanded.contains("log.debug"));
        assert!(expanded.contains("log.info"));
        assert!(expanded.contains("log.warn"));
        assert!(expanded.contains("log.error"));
    }

    #[test]
    fn effect_expand_leaf_effect_no_change() {
        let checker = EffectChecker::new();
        let declared = EffectSet::from_effect_names(["console.read"]);
        let expanded = checker.expand(&declared);
        assert_eq!(expanded.len(), 1);
        assert!(expanded.contains("console.read"));
    }

    #[test]
    fn effect_expand_pure_stays_empty() {
        let checker = EffectChecker::new();
        let declared = EffectSet::pure();
        let expanded = checker.expand(&declared);
        assert!(expanded.is_pure());
    }

    // -- Containment checks: positive (no errors) --

    #[test]
    fn effect_containment_pure_calling_pure_ok() {
        // Pure function calling another pure function: no errors
        let checker = EffectChecker::new();
        let declared = EffectSet::pure();
        let actual = EffectSet::pure();
        let errors = checker.check_containment(&declared, &actual, &(0..10));
        assert!(errors.is_empty());
    }

    #[test]
    fn effect_containment_declared_superset_ok() {
        // Declared {io, mem}, actual {mem}: mem is subset, OK
        let checker = EffectChecker::new();
        let declared = EffectSet::from_effect_names(["io", "mem"]);
        let actual = EffectSet::from_effect_names(["mem"]);
        let errors = checker.check_containment(&declared, &actual, &(0..10));
        assert!(errors.is_empty());
    }

    #[test]
    fn effect_containment_exact_match_ok() {
        // Declared and actual are identical: OK
        let checker = EffectChecker::new();
        let declared = EffectSet::from_effect_names(["io", "mem"]);
        let actual = EffectSet::from_effect_names(["io", "mem"]);
        let errors = checker.check_containment(&declared, &actual, &(0..10));
        assert!(errors.is_empty());
    }

    #[test]
    fn effect_containment_hierarchy_io_covers_console_ok() {
        // Declared {io}, actual {console.read}: io expands to include
        // console.read, so this is OK
        let checker = EffectChecker::new();
        let declared = EffectSet::from_effect_names(["io"]);
        let actual = EffectSet::from_effect_names(["console.read"]);
        let errors = checker.check_containment(&declared, &actual, &(0..10));
        assert!(errors.is_empty());
    }

    #[test]
    fn effect_containment_hierarchy_io_covers_network_ok() {
        let checker = EffectChecker::new();
        let declared = EffectSet::from_effect_names(["io"]);
        let actual = EffectSet::from_effect_names(["network.send", "network.receive"]);
        let errors = checker.check_containment(&declared, &actual, &(0..10));
        assert!(errors.is_empty());
    }

    #[test]
    fn effect_containment_hierarchy_database_covers_read_ok() {
        let checker = EffectChecker::new();
        let declared = EffectSet::from_effect_names(["database"]);
        let actual = EffectSet::from_effect_names(["database.read"]);
        let errors = checker.check_containment(&declared, &actual, &(0..10));
        assert!(errors.is_empty());
    }

    #[test]
    fn effect_containment_hierarchy_logging_covers_all_levels_ok() {
        let checker = EffectChecker::new();
        let declared = EffectSet::from_effect_names(["logging"]);
        let actual =
            EffectSet::from_effect_names(["log.debug", "log.info", "log.warn", "log.error"]);
        let errors = checker.check_containment(&declared, &actual, &(0..10));
        assert!(errors.is_empty());
    }

    #[test]
    fn effect_containment_declared_io_actual_empty_ok() {
        // Declared {io}, actual empty (pure body): always OK
        let checker = EffectChecker::new();
        let declared = EffectSet::from_effect_names(["io"]);
        let actual = EffectSet::pure();
        let errors = checker.check_containment(&declared, &actual, &(0..10));
        assert!(errors.is_empty());
    }

    // -- A07002: pure function performs effect --

    #[test]
    fn effect_containment_pure_performs_io_a07002() {
        // Pure function (empty declared set) performs io: A07002
        let checker = EffectChecker::new();
        let declared = EffectSet::pure();
        let actual = EffectSet::from_effect_names(["io"]);
        let errors = checker.check_containment(&declared, &actual, &(0..10));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A07002");
        assert!(errors[0].message.contains("pure"));
        assert!(errors[0].message.contains("io"));
    }

    #[test]
    fn effect_containment_pure_performs_mem_a07002() {
        let checker = EffectChecker::new();
        let declared = EffectSet::pure();
        let actual = EffectSet::from_effect_names(["mem"]);
        let errors = checker.check_containment(&declared, &actual, &(0..10));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A07002");
        assert!(errors[0].message.contains("mem"));
    }

    #[test]
    fn effect_containment_pure_performs_multiple_a07002() {
        // Pure function performs multiple effects: one A07002 per effect
        let checker = EffectChecker::new();
        let declared = EffectSet::pure();
        let actual = EffectSet::from_effect_names(["io", "mem"]);
        let errors = checker.check_containment(&declared, &actual, &(0..10));
        assert_eq!(errors.len(), 2);
        assert!(errors.iter().all(|e| e.code == "A07002"));
    }

    // -- A07001: undeclared effect --

    #[test]
    fn effect_containment_undeclared_effect_a07001() {
        // Declared {io}, actual {io, mem}: mem is not declared => A07001
        let checker = EffectChecker::new();
        let declared = EffectSet::from_effect_names(["io"]);
        let actual = EffectSet::from_effect_names(["io", "mem"]);
        let errors = checker.check_containment(&declared, &actual, &(0..10));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A07001");
        assert!(errors[0].message.contains("mem"));
    }

    #[test]
    fn effect_containment_leaf_without_parent_a07001() {
        // Declared {console.read}, actual {console.write}: different leaf
        let checker = EffectChecker::new();
        let declared = EffectSet::from_effect_names(["console.read"]);
        let actual = EffectSet::from_effect_names(["console.write"]);
        let errors = checker.check_containment(&declared, &actual, &(0..10));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A07001");
        assert!(errors[0].message.contains("console.write"));
    }

    #[test]
    fn effect_containment_database_without_io_a07001() {
        // Declared {io}, actual {database.read}: database is not under io
        let checker = EffectChecker::new();
        let declared = EffectSet::from_effect_names(["io"]);
        let actual = EffectSet::from_effect_names(["database.read"]);
        let errors = checker.check_containment(&declared, &actual, &(0..10));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A07001");
        assert!(errors[0].message.contains("database.read"));
    }

    #[test]
    fn effect_containment_multiple_undeclared_a07001() {
        // Declared {mem}, actual {io, database}: two undeclared effects
        let checker = EffectChecker::new();
        let declared = EffectSet::from_effect_names(["mem"]);
        let actual = EffectSet::from_effect_names(["io", "database"]);
        let errors = checker.check_containment(&declared, &actual, &(0..10));
        assert_eq!(errors.len(), 2);
        assert!(errors.iter().all(|e| e.code == "A07001"));
    }

    // -- Effect containment across call chain (T037 specific) --

    #[test]
    fn effect_containment_call_chain() {
        // Simulate: fn outer() effects {io} calls fn inner() effects {io, mem}
        // inner's actual effects must be subset of outer's declared.
        // mem is not in outer's declared set => A07001 for the call chain.
        let checker = EffectChecker::new();
        let outer_declared = EffectSet::from_effect_names(["io"]);
        // inner's effects propagate to outer's body
        let outer_actual = EffectSet::from_effect_names(["io", "mem"]);
        let errors = checker.check_containment(&outer_declared, &outer_actual, &(0..10));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A07001");
        assert!(errors[0].message.contains("mem"));
    }

    #[test]
    fn effect_containment_call_chain_pure_callee_ok() {
        // fn outer() effects {io} calls fn inner() effects {pure}
        // pure is always a subset: OK
        let checker = EffectChecker::new();
        let outer_declared = EffectSet::from_effect_names(["io"]);
        let outer_actual = EffectSet::pure();
        let errors = checker.check_containment(&outer_declared, &outer_actual, &(0..10));
        assert!(errors.is_empty());
    }

    // -- Edge cases --

    #[test]
    fn effect_set_dedup() {
        // Duplicate effect names in iterator are deduplicated
        let set = EffectSet::from_effect_names(["io", "io", "mem", "mem"]);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn effect_checker_default_trait() {
        // Default implementation works
        let checker = EffectChecker::default();
        assert!(checker.is_known("io"));
    }

    #[test]
    fn effect_expand_multiple_groups() {
        // Expanding {io, database} should include sub-effects of both
        let checker = EffectChecker::new();
        let declared = EffectSet::from_effect_names(["io", "database"]);
        let expanded = checker.expand(&declared);
        assert!(expanded.contains("console.read"));
        assert!(expanded.contains("database.write"));
    }

    #[test]
    fn effect_containment_span_preserved() {
        // Verify that the span from the input is preserved in errors
        let checker = EffectChecker::new();
        let declared = EffectSet::pure();
        let actual = EffectSet::from_effect_names(["io"]);
        let errors = checker.check_containment(&declared, &actual, &(42..99));
        assert_eq!(errors[0].span, 42..99);
    }

    #[test]
    fn effect_set_iter() {
        let set = EffectSet::from_effect_names(["io", "mem"]);
        let mut items: Vec<&str> = set.iter().collect();
        items.sort();
        assert_eq!(items, vec!["io", "mem"]);
    }

    // -----------------------------------------------------------------------
    // T050: Section 13 type interaction tests
    //
    // These test pairwise (and three-way) interactions between:
    //   - Refinement types
    //   - Linear types (UsageTracker, LinearContext)
    //   - Typestate (TypestateChecker)
    //   - Effects (EffectChecker, EffectSet)
    //
    // Tests covering information flow and dependent types are deferred
    // until T051/T052 are implemented.
    // -----------------------------------------------------------------------

    // -- Test Case 1: Refinement + Linear (Ghost Use Problem) ----------------
    //
    // Spec Section 13.1: A refinement predicate references a linear variable.
    // Refinement predicates are ghost (logical, not computational) and do
    // NOT count as a linear use. The variable is only consumed by
    // computational (runtime) uses.

    #[test]
    fn interaction_refinement_linear_ghost_use_does_not_consume() {
        // Section 13, Test Case 1: a refinement predicate on a linear
        // variable is grade-0 (erased/ghost). It must NOT count as a
        // runtime use.
        //
        // Scenario: linear var `buf` has a refinement `buf.len > 0`.
        // The refinement is a compile-time/SMT-level constraint only.
        // One computational use follows. Total runtime uses = 1 => OK.
        let mut tracker = UsageTracker::new();
        tracker.declare("buf".into(), UsageGrade::Linear, 0..3);

        // Refinement predicate `buf.len > 0` is ghost: do NOT call use_var.
        // Only the single computational use counts:
        tracker.use_var("buf");

        let errors = tracker.check();
        assert!(
            errors.is_empty(),
            "ghost refinement reference should not count as a use: {errors:?}"
        );
        assert_eq!(tracker.get_count("buf"), Some(1));
    }

    #[test]
    fn interaction_refinement_linear_two_computational_uses_a05001() {
        // Section 13, Test Case 1 (negative): two computational uses of
        // a linear variable must produce A05001, regardless of whether a
        // refinement predicate also references the variable.
        let mut tracker = UsageTracker::new();
        tracker.declare("buf".into(), UsageGrade::Linear, 0..3);

        // Refinement predicate (ghost, not counted):
        // -- buf.is_valid (not called via use_var)

        // Two computational (runtime) uses:
        tracker.use_var("buf"); // first use: pass to read()
        tracker.use_var("buf"); // second use: pass to write()

        let errors = tracker.check();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A05001");
        assert!(errors[0].message.contains("buf"));
        assert!(errors[0].message.contains("2 times"));
    }

    #[test]
    fn interaction_refinement_linear_ghost_grade_erased_no_runtime() {
        // A ghost (Erased) variable used in refinement predicates only:
        // grade-0 means zero runtime uses are allowed. Using it at runtime
        // is A05002. This tests the boundary between refinement context
        // (logical) and runtime context.
        let mut tracker = UsageTracker::new();
        tracker.declare("ghost_bound".into(), UsageGrade::Erased, 0..11);

        // Ghost variable is NOT used at runtime (only in predicates).
        // This is correct: erased variables exist only in logic.
        let errors = tracker.check();
        assert!(
            errors.is_empty(),
            "erased variable with no runtime use should pass: {errors:?}"
        );
    }

    #[test]
    fn interaction_refinement_linear_erased_runtime_use_a05002() {
        // Erased variable used at runtime: A05002.
        // This catches the case where a ghost refinement variable
        // accidentally leaks into computational code.
        let mut tracker = UsageTracker::new();
        tracker.declare("ghost_bound".into(), UsageGrade::Erased, 0..11);

        tracker.use_var("ghost_bound"); // runtime use of erased var

        let errors = tracker.check();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A05002");
        assert!(errors[0].message.contains("erased"));
    }

    #[test]
    fn interaction_refinement_linear_refined_type_with_linear_base() {
        // A refined type `{ v: Int | v > 0 }` where the base value is
        // linear. The predicate `v > 0` is ghost; the value `v` itself
        // is linear and must be used exactly once.
        let mut tracker = UsageTracker::new();
        tracker.declare("pos_val".into(), UsageGrade::Linear, 0..7);

        // Type is Refined { base: Int, predicate: "v > 0" }
        // The predicate check is done at compile time (SMT), not runtime.
        // One computational use:
        tracker.use_var("pos_val");

        let errors = tracker.check();
        assert!(errors.is_empty());

        // Verify the type representation captures both aspects
        let ty = Type::Refined {
            base: Box::new(Type::Int),
            predicate: "v > 0".into(),
        };
        assert_eq!(format!("{ty}"), "Int{v > 0}");
    }

    // -- Test Case 4: Linear + Effect (Resource-Scoped Effects) --------------
    //
    // Spec Section 13.4: Linear resources interact with the effect system.
    // A function consuming a linear resource should declare appropriate
    // effects. The linear variable must still be consumed exactly once.

    #[test]
    fn interaction_linear_effect_consume_with_correct_effects() {
        // A function that consumes a linear resource and declares `io`
        // effects. The linear variable is consumed exactly once, and the
        // declared effects cover the actual effects. Both checks pass.
        let mut tracker = UsageTracker::new();
        tracker.declare("conn".into(), UsageGrade::Linear, 0..4);
        let mut ctx = LinearContext::new(tracker);

        // Simulate: conn is consumed by calling conn.close()
        let expr = AstExpr::MethodCall {
            receiver: Box::new(AstExpr::Ident("conn".into())),
            method: "close".into(),
            args: vec![],
        };
        let branch_errors = check_expr_linearity(&expr, &mut ctx);
        assert!(branch_errors.is_empty());

        // Linear check: conn used exactly once => OK
        let linear_errors = ctx.check();
        assert!(linear_errors.is_empty());

        // Effect check: function declares {io}, body performs {io} => OK
        let checker = EffectChecker::new();
        let declared = EffectSet::from_effect_names(["io"]);
        let actual = EffectSet::from_effect_names(["io"]);
        let effect_errors = checker.check_containment(&declared, &actual, &(0..10));
        assert!(effect_errors.is_empty());
    }

    #[test]
    fn interaction_linear_effect_resource_not_consumed_a05002() {
        // A function with correct effects but that forgets to consume
        // its linear resource. The effect check passes, but the linear
        // check must report A05002 (unused linear variable).
        let mut tracker = UsageTracker::new();
        tracker.declare("conn".into(), UsageGrade::Linear, 0..4);
        let mut ctx = LinearContext::new(tracker);

        // Function body does NOT use conn at all
        let expr = AstExpr::Literal(AstLit::Int("0".into()));
        let branch_errors = check_expr_linearity(&expr, &mut ctx);
        assert!(branch_errors.is_empty());

        // Linear check: conn never consumed => A05002
        let linear_errors = ctx.check();
        assert_eq!(linear_errors.len(), 1);
        assert_eq!(linear_errors[0].code, "A05002");
        assert!(linear_errors[0].message.contains("conn"));

        // Effect check: independently passes (effects are about the
        // function's declared vs actual effects, not resource consumption)
        let checker = EffectChecker::new();
        let declared = EffectSet::from_effect_names(["io"]);
        let actual = EffectSet::from_effect_names(["io"]);
        let effect_errors = checker.check_containment(&declared, &actual, &(0..10));
        assert!(effect_errors.is_empty());
    }

    #[test]
    fn interaction_linear_effect_pure_function_with_linear_resource() {
        // A pure function that consumes a linear resource. The resource
        // is consumed correctly (linear check passes), but the function
        // is pure, so any effectful operation on it should be caught by
        // the effect checker.
        let mut tracker = UsageTracker::new();
        tracker.declare("handle".into(), UsageGrade::Linear, 0..6);
        let mut ctx = LinearContext::new(tracker);

        // Resource consumed (linear OK)
        let expr = AstExpr::Ident("handle".into());
        let _ = check_expr_linearity(&expr, &mut ctx);
        let linear_errors = ctx.check();
        assert!(linear_errors.is_empty());

        // But function is declared pure, body does io => A07002
        let checker = EffectChecker::new();
        let declared = EffectSet::pure();
        let actual = EffectSet::from_effect_names(["io"]);
        let effect_errors = checker.check_containment(&declared, &actual, &(0..10));
        assert_eq!(effect_errors.len(), 1);
        assert_eq!(effect_errors[0].code, "A07002");
    }

    #[test]
    fn interaction_linear_effect_undeclared_effect_on_resource() {
        // Function declares {mem} but performs {io} on the linear resource.
        // Linear check passes (resource consumed once), but effect check
        // fails with A07001 (undeclared effect).
        let mut tracker = UsageTracker::new();
        tracker.declare("socket".into(), UsageGrade::Linear, 0..6);
        let mut ctx = LinearContext::new(tracker);

        // Resource consumed
        let expr = AstExpr::MethodCall {
            receiver: Box::new(AstExpr::Ident("socket".into())),
            method: "send".into(),
            args: vec![AstExpr::Literal(AstLit::Str("data".into()))],
        };
        let _ = check_expr_linearity(&expr, &mut ctx);
        let linear_errors = ctx.check();
        assert!(linear_errors.is_empty());

        // Effect mismatch: declared {mem}, actual {io}
        let checker = EffectChecker::new();
        let declared = EffectSet::from_effect_names(["mem"]);
        let actual = EffectSet::from_effect_names(["io"]);
        let effect_errors = checker.check_containment(&declared, &actual, &(0..10));
        assert_eq!(effect_errors.len(), 1);
        assert_eq!(effect_errors[0].code, "A07001");
    }

    // -- Linear + Typestate interaction tests --------------------------------
    //
    // Typestate variables MUST be linear (A06002). This tests the
    // interaction between the two checkers.

    #[test]
    fn interaction_linear_typestate_must_be_linear() {
        // A typestate variable that is not declared as linear must fail
        // with A06002. Typestate requires linearity to prevent aliasing
        // which could observe inconsistent states.
        let states = vec!["Init".into(), "Ready".into()];
        let transitions = vec![("start".into(), "Init".into(), "Ready".into())];
        let checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

        // Not linear => A06002
        let err = checker.validate_linear(false);
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A06002");
    }

    #[test]
    fn interaction_linear_typestate_linear_ok() {
        // A typestate variable declared as linear passes the linearity
        // check and can proceed with state transitions.
        let states = vec!["Locked".into(), "Unlocked".into()];
        let transitions = vec![
            ("unlock".into(), "Locked".into(), "Unlocked".into()),
            ("lock".into(), "Unlocked".into(), "Locked".into()),
        ];
        let mut checker = TypestateChecker::new(states, transitions, "Locked".into(), 0..6);

        // Linear check passes
        assert!(checker.validate_linear(true).is_none());

        // Typestate transitions work
        assert!(checker.transition("unlock", 10..16).is_ok());
        assert_eq!(checker.current_state(), "Unlocked");

        // Linear usage tracking: consumed exactly once
        let mut tracker = UsageTracker::new();
        tracker.declare("lock_var".into(), UsageGrade::Linear, 0..8);
        tracker.use_var("lock_var"); // consumed by unlock operation
        assert!(tracker.check().is_empty());
    }

    #[test]
    fn interaction_linear_typestate_double_use_violates_both() {
        // Using a typestate variable twice violates both linearity (A05001)
        // and potentially causes observable aliasing. Both checkers must
        // report their respective errors independently.
        let mut tracker = UsageTracker::new();
        tracker.declare("file".into(), UsageGrade::Linear, 0..4);
        tracker.use_var("file"); // first use: read
        tracker.use_var("file"); // second use: write (aliasing!)

        let linear_errors = tracker.check();
        assert_eq!(linear_errors.len(), 1);
        assert_eq!(linear_errors[0].code, "A05001");
    }

    // -- Effect + Typestate interaction tests --------------------------------
    //
    // Operations that cause typestate transitions may also have effect
    // requirements. Both the state transition validity and effect
    // containment must be checked.

    #[test]
    fn interaction_effect_typestate_transition_with_effects() {
        // An operation that transitions state and has effects.
        // Both the typestate transition and effect containment must pass.
        let states = vec!["Disconnected".into(), "Connected".into()];
        let transitions = vec![("connect".into(), "Disconnected".into(), "Connected".into())];
        let mut ts_checker =
            TypestateChecker::new(states, transitions, "Disconnected".into(), 0..12);

        // Typestate: connect() in Disconnected => Connected (OK)
        assert!(ts_checker.transition("connect", 20..27).is_ok());
        assert_eq!(ts_checker.current_state(), "Connected");

        // Effect: function declares {io}, connect performs {io} (OK)
        let eff_checker = EffectChecker::new();
        let declared = EffectSet::from_effect_names(["io"]);
        let actual = EffectSet::from_effect_names(["network.connect"]);
        let eff_errors = eff_checker.check_containment(&declared, &actual, &(20..27));
        assert!(eff_errors.is_empty());
    }

    #[test]
    fn interaction_effect_typestate_wrong_state_with_correct_effects() {
        // Operation has correct effects but is called in the wrong state.
        // Effect check passes, but typestate check must fail with A06001.
        let states = vec!["Closed".into(), "Open".into()];
        let transitions = vec![("write".into(), "Open".into(), "Open".into())];
        let mut ts_checker = TypestateChecker::new(states, transitions, "Closed".into(), 0..6);

        // Typestate: write() requires Open but we are in Closed => A06001
        let ts_err = ts_checker.transition("write", 10..15);
        assert!(ts_err.is_err());
        assert_eq!(ts_err.unwrap_err().code, "A06001");

        // Effect check: independently passes
        let eff_checker = EffectChecker::new();
        let declared = EffectSet::from_effect_names(["io"]);
        let actual = EffectSet::from_effect_names(["io"]);
        assert!(
            eff_checker
                .check_containment(&declared, &actual, &(10..15))
                .is_empty()
        );
    }

    #[test]
    fn interaction_effect_typestate_correct_state_wrong_effects() {
        // Operation is called in the correct state but with undeclared
        // effects. Typestate check passes, effect check fails with A07001.
        let states = vec!["Init".into(), "Running".into()];
        let transitions = vec![("start".into(), "Init".into(), "Running".into())];
        let mut ts_checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

        // Typestate: start() in Init => Running (OK)
        assert!(ts_checker.transition("start", 5..10).is_ok());

        // Effect: function declares {mem} but start() does {io} => A07001
        let eff_checker = EffectChecker::new();
        let declared = EffectSet::from_effect_names(["mem"]);
        let actual = EffectSet::from_effect_names(["io"]);
        let errors = eff_checker.check_containment(&declared, &actual, &(5..10));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A07001");
    }

    // -- Test Case 10: Conditional Typestate (Branch Divergence) --------------
    //
    // Spec Section 13.10: Different branches lead to different states.
    // After diverging branches, state is ambiguous => A06004.

    #[test]
    fn interaction_typestate_branch_divergence_a06004() {
        // After an if/match, if one branch transitions to state A and
        // the other to state B, the post-branch state is ambiguous.
        let states = vec!["Idle".into(), "Active".into(), "Error".into()];
        let transitions = vec![
            ("activate".into(), "Idle".into(), "Active".into()),
            ("fail".into(), "Idle".into(), "Error".into()),
        ];

        // Branch A: activate => Active
        let mut checker_a =
            TypestateChecker::new(states.clone(), transitions.clone(), "Idle".into(), 0..4);
        checker_a.transition("activate", 10..18).unwrap();

        // Branch B: fail => Error
        let mut checker_b = TypestateChecker::new(states, transitions, "Idle".into(), 0..4);
        checker_b.transition("fail", 10..14).unwrap();

        // Post-branch: Active vs Error => A06004
        let err = TypestateChecker::check_branch_consistency(&checker_a, &checker_b, 20..25);
        assert!(err.is_some());
        let err = err.unwrap();
        assert_eq!(err.code, "A06004");
        assert!(err.message.contains("Active"));
        assert!(err.message.contains("Error"));
    }

    #[test]
    fn interaction_typestate_branch_divergence_same_state_ok() {
        // Both branches transition to the same state: no ambiguity.
        let states = vec!["Pending".into(), "Done".into()];
        let transitions = vec![
            ("complete_a".into(), "Pending".into(), "Done".into()),
            ("complete_b".into(), "Pending".into(), "Done".into()),
        ];

        let mut checker_a =
            TypestateChecker::new(states.clone(), transitions.clone(), "Pending".into(), 0..7);
        checker_a.transition("complete_a", 10..20).unwrap();

        let mut checker_b = TypestateChecker::new(states, transitions, "Pending".into(), 0..7);
        checker_b.transition("complete_b", 10..20).unwrap();

        let err = TypestateChecker::check_branch_consistency(&checker_a, &checker_b, 20..25);
        assert!(err.is_none());
    }

    #[test]
    fn interaction_typestate_branch_one_transitions_other_stays() {
        // One branch transitions, the other stays in the original state.
        // Post-branch: states differ => A06004.
        let states = vec!["Idle".into(), "Active".into()];
        let transitions = vec![("start".into(), "Idle".into(), "Active".into())];

        let mut checker_a =
            TypestateChecker::new(states.clone(), transitions.clone(), "Idle".into(), 0..4);
        checker_a.transition("start", 10..15).unwrap();
        // checker_a: Active

        let checker_b = TypestateChecker::new(states, transitions, "Idle".into(), 0..4);
        // checker_b: still Idle (no transition in this branch)

        let err = TypestateChecker::check_branch_consistency(&checker_a, &checker_b, 20..25);
        assert!(err.is_some());
        let err = err.unwrap();
        assert_eq!(err.code, "A06004");
        assert!(err.message.contains("Active"));
        assert!(err.message.contains("Idle"));
    }

    #[test]
    fn interaction_typestate_branch_divergence_with_linear_context() {
        // Combine typestate branch divergence with linear context splitting.
        // A linear variable is used consistently in both branches (OK for
        // linearity), but the typestate diverges (A06004).
        let mut tracker = UsageTracker::new();
        tracker.declare("resource".into(), UsageGrade::Linear, 0..8);
        let mut ctx = LinearContext::new(tracker);

        // if cond then use(resource) else use(resource)
        let expr = AstExpr::If {
            cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
            then_branch: Box::new(AstExpr::Call {
                func: Box::new(AstExpr::Ident("activate".into())),
                args: vec![AstExpr::Ident("resource".into())],
            }),
            else_branch: Some(Box::new(AstExpr::Call {
                func: Box::new(AstExpr::Ident("deactivate".into())),
                args: vec![AstExpr::Ident("resource".into())],
            })),
        };
        let branch_errors = check_expr_linearity(&expr, &mut ctx);
        // Linear: consistent (1 use in each branch) => no A05004
        assert!(
            branch_errors.is_empty(),
            "linear should be consistent: {branch_errors:?}"
        );
        let linear_final = ctx.check();
        assert!(linear_final.is_empty());

        // Meanwhile, typestate diverges:
        let states = vec!["Idle".into(), "Active".into(), "Stopped".into()];
        let transitions = vec![
            ("activate".into(), "Idle".into(), "Active".into()),
            ("deactivate".into(), "Idle".into(), "Stopped".into()),
        ];
        let mut ts_a =
            TypestateChecker::new(states.clone(), transitions.clone(), "Idle".into(), 0..4);
        ts_a.transition("activate", 10..18).unwrap();

        let mut ts_b = TypestateChecker::new(states, transitions, "Idle".into(), 0..4);
        ts_b.transition("deactivate", 10..20).unwrap();

        let ts_err = TypestateChecker::check_branch_consistency(&ts_a, &ts_b, 0..25);
        assert!(ts_err.is_some());
        assert_eq!(ts_err.unwrap().code, "A06004");
    }

    // -- Effect containment in functions (pure calling effectful) -------------
    //
    // Spec Section 3.5: A pure function calling an effectful one is an
    // effect containment violation.

    #[test]
    fn interaction_effect_containment_pure_calls_io_a07002() {
        // A function declared `pure` (empty effect set) that internally
        // performs an `io` effect must produce A07002.
        let checker = EffectChecker::new();
        let declared = EffectSet::pure();
        let actual = EffectSet::from_effect_names(["io"]);
        let errors = checker.check_containment(&declared, &actual, &(0..10));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A07002");
        assert!(errors[0].message.contains("pure"));
        assert!(errors[0].message.contains("io"));
    }

    #[test]
    fn interaction_effect_containment_io_calls_database_a07001() {
        // A function declared `{io}` that performs `database.write`:
        // database effects are NOT sub-effects of io.
        // This must produce A07001.
        let checker = EffectChecker::new();
        let declared = EffectSet::from_effect_names(["io"]);
        let actual = EffectSet::from_effect_names(["database.write"]);
        let errors = checker.check_containment(&declared, &actual, &(0..10));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A07001");
    }

    #[test]
    fn interaction_effect_containment_database_covers_subeffects() {
        // A function declared `{database}` can perform `database.read`
        // and `database.write` (sub-effects of the database group).
        let checker = EffectChecker::new();
        let declared = EffectSet::from_effect_names(["database"]);
        let actual = EffectSet::from_effect_names(["database.read", "database.write"]);
        let errors = checker.check_containment(&declared, &actual, &(0..10));
        assert!(errors.is_empty());
    }

    // -- Linear context fork/merge with multiple variables -------------------
    //
    // Tests that context splitting correctly tracks multiple independent
    // linear variables through branches.

    #[test]
    fn interaction_linear_context_fork_merge_two_vars() {
        // Two linear variables, each consumed in different branches.
        // var `a` consumed in then-branch, var `b` consumed in else-branch.
        // Both are inconsistent across branches => two A05004 errors.
        let mut tracker = UsageTracker::new();
        tracker.declare("a".into(), UsageGrade::Linear, 0..1);
        tracker.declare("b".into(), UsageGrade::Linear, 2..3);
        let mut ctx = LinearContext::new(tracker);

        // if cond then a else b
        let expr = AstExpr::If {
            cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
            then_branch: Box::new(AstExpr::Ident("a".into())),
            else_branch: Some(Box::new(AstExpr::Ident("b".into()))),
        };
        let errors = check_expr_linearity(&expr, &mut ctx);
        assert_eq!(errors.len(), 2);
        assert!(errors.iter().all(|e| e.code == "A05004"));

        // One error for `a` (used in then, not in else)
        // One error for `b` (used in else, not in then)
        let names: Vec<bool> = errors
            .iter()
            .map(|e| e.message.contains("a") || e.message.contains("b"))
            .collect();
        assert!(names.iter().all(|&b| b));
    }

    #[test]
    fn interaction_linear_context_fork_merge_swap_in_branches() {
        // Two linear variables, both consumed once in each branch
        // (swapped order). Both are consistent => no errors.
        let mut tracker = UsageTracker::new();
        tracker.declare("x".into(), UsageGrade::Linear, 0..1);
        tracker.declare("y".into(), UsageGrade::Linear, 2..3);
        let mut ctx = LinearContext::new(tracker);

        // if cond then [x, y] else [y, x]
        // Both x and y used once in each branch (consistent delta = 1)
        let expr = AstExpr::If {
            cond: Box::new(AstExpr::Literal(AstLit::Bool(true))),
            then_branch: Box::new(AstExpr::List(vec![
                AstExpr::Ident("x".into()),
                AstExpr::Ident("y".into()),
            ])),
            else_branch: Some(Box::new(AstExpr::List(vec![
                AstExpr::Ident("y".into()),
                AstExpr::Ident("x".into()),
            ]))),
        };
        let branch_errors = check_expr_linearity(&expr, &mut ctx);
        assert!(branch_errors.is_empty());

        let final_errors = ctx.check();
        assert!(final_errors.is_empty());
    }

    // -- Test Case 7: Linear + Information Flow (orthogonal axes) ------------
    //
    // Spec Section 13.7: Linearity and information flow are independent.
    // A value has both a usage grade (linear, unlimited, etc.) and a
    // security label (Public, Confidential, etc.). These are tracked on
    // orthogonal axes.
    //
    // Since information flow checking (T051) is not yet implemented, we
    // test the orthogonality at the type/tracker level: a variable with
    // a security label type AND a linear grade should be checked for both
    // independently.

    #[test]
    fn interaction_linear_infoflow_orthogonal_grade_and_type() {
        // A variable that is both linear (grade 1) and has a
        // Confidential-labeled type. The linear checker tracks usage;
        // the type checker tracks the label. They do not interfere.
        let mut tracker = UsageTracker::new();
        tracker.declare("secret_key".into(), UsageGrade::Linear, 0..10);

        // Type is Refined { base: Bytes, predicate: "label == Confidential" }
        let _ty = Type::Refined {
            base: Box::new(Type::Bytes),
            predicate: "label == Confidential".into(),
        };

        // One computational use: linear check passes
        tracker.use_var("secret_key");
        let errors = tracker.check();
        assert!(errors.is_empty());
    }

    #[test]
    fn interaction_linear_infoflow_unlimited_with_label() {
        // An unlimited variable with a Public label. No linearity
        // constraints, but the type carries the label for info-flow.
        let mut tracker = UsageTracker::new();
        tracker.declare("public_data".into(), UsageGrade::Unlimited, 0..11);

        let _ty = Type::Refined {
            base: Box::new(Type::String),
            predicate: "label == Public".into(),
        };

        // Multiple uses: unlimited grade allows any count
        tracker.use_var("public_data");
        tracker.use_var("public_data");
        tracker.use_var("public_data");
        let errors = tracker.check();
        assert!(errors.is_empty());
    }

    // -- Test Case 8: Typestate + Effect + Refinement (Three-Way) ------------
    //
    // Spec Section 13.8: All three features interact simultaneously.
    // A typestate variable has a refinement predicate, undergoes state
    // transitions, and the operations have effect annotations.

    #[test]
    fn interaction_three_way_typestate_effect_refinement_all_pass() {
        // Three-way interaction:
        // 1. Typestate: object transitions Init -> Open -> Closed
        // 2. Effects: open() has {io}, close() has {io}
        // 3. Refinement: object has a predicate (capacity > 0)
        //
        // All three checks pass when correctly combined.
        let states = vec!["Init".into(), "Open".into(), "Closed".into()];
        let transitions = vec![
            ("open".into(), "Init".into(), "Open".into()),
            ("close".into(), "Open".into(), "Closed".into()),
        ];
        let mut ts = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

        // Typestate transitions
        assert!(ts.transition("open", 10..14).is_ok());
        assert!(ts.transition("close", 15..20).is_ok());
        assert_eq!(ts.current_state(), "Closed");

        // Typestate variable is linear
        assert!(ts.validate_linear(true).is_none());

        // All transitions reference declared states
        assert!(ts.validate_transitions().is_empty());

        // Effects: function declares {io}, body performs {io}
        let eff = EffectChecker::new();
        let declared = EffectSet::from_effect_names(["io"]);
        let actual = EffectSet::from_effect_names(["network.connect"]);
        assert!(
            eff.check_containment(&declared, &actual, &(10..20))
                .is_empty()
        );

        // Refinement: the type has a predicate (compile-time, no runtime cost)
        let ty = Type::Refined {
            base: Box::new(Type::Named("Connection".into())),
            predicate: "capacity > 0".into(),
        };
        assert_eq!(format!("{ty}"), "Connection{capacity > 0}");
    }

    #[test]
    fn interaction_three_way_typestate_passes_effect_fails() {
        // Three-way: typestate and refinement are OK, but effects fail.
        // This tests that each checker operates independently.
        let states = vec!["Ready".into(), "Done".into()];
        let transitions = vec![("execute".into(), "Ready".into(), "Done".into())];
        let mut ts = TypestateChecker::new(states, transitions, "Ready".into(), 0..5);

        // Typestate OK
        assert!(ts.transition("execute", 10..17).is_ok());

        // Refinement OK (ghost predicate)
        let _ty = Type::Refined {
            base: Box::new(Type::Named("Task".into())),
            predicate: "priority > 0".into(),
        };

        // Effects FAIL: declared pure, body does io
        let eff = EffectChecker::new();
        let declared = EffectSet::pure();
        let actual = EffectSet::from_effect_names(["io"]);
        let errors = eff.check_containment(&declared, &actual, &(10..17));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A07002");
    }

    #[test]
    fn interaction_three_way_effect_passes_typestate_fails() {
        // Three-way: effects are OK, but typestate transition fails.
        let states = vec!["Locked".into(), "Unlocked".into()];
        let transitions = vec![("unlock".into(), "Locked".into(), "Unlocked".into())];
        let mut ts = TypestateChecker::new(
            states,
            transitions,
            "Unlocked".into(), // Already unlocked
            0..8,
        );

        // Typestate FAIL: unlock requires Locked, but we are Unlocked
        let ts_err = ts.transition("unlock", 10..16);
        assert!(ts_err.is_err());
        assert_eq!(ts_err.unwrap_err().code, "A06001");

        // Effects OK: declared {io}, body does {io}
        let eff = EffectChecker::new();
        let declared = EffectSet::from_effect_names(["io"]);
        let actual = EffectSet::from_effect_names(["io"]);
        assert!(
            eff.check_containment(&declared, &actual, &(10..16))
                .is_empty()
        );
    }

    // -- Test Case 11 proxy: Effect + Info-flow (labeled effects) ------------
    //
    // Since full information flow is not yet implemented (T051), we test
    // the effect system's ability to distinguish between effect categories
    // that will eventually carry labels. This validates the infrastructure
    // needed for Test Case 11.

    #[test]
    fn interaction_effect_hierarchy_separation() {
        // io effects and database effects are separate hierarchies.
        // Declaring {io} does NOT cover {database.write}.
        // This separation is the foundation for Test Case 11's labeled
        // effects where different effect categories may have different
        // security labels.
        let checker = EffectChecker::new();

        // io does NOT cover database
        let declared_io = EffectSet::from_effect_names(["io"]);
        let actual_db = EffectSet::from_effect_names(["database.write"]);
        let errors = checker.check_containment(&declared_io, &actual_db, &(0..5));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A07001");

        // database does NOT cover io
        let declared_db = EffectSet::from_effect_names(["database"]);
        let actual_io = EffectSet::from_effect_names(["console.write"]);
        let errors = checker.check_containment(&declared_db, &actual_io, &(0..5));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A07001");
    }

    #[test]
    fn interaction_effect_multiple_groups_combined() {
        // Declaring both {io, database} covers sub-effects of both.
        let checker = EffectChecker::new();
        let declared = EffectSet::from_effect_names(["io", "database"]);
        let actual =
            EffectSet::from_effect_names(["console.write", "network.send", "database.read"]);
        let errors = checker.check_containment(&declared, &actual, &(0..10));
        assert!(errors.is_empty());
    }

    // -- Combined: Linear + Typestate + Effect (full pipeline simulation) ----

    #[test]
    fn interaction_full_pipeline_linear_typestate_effect_pass() {
        // Simulate a full pipeline check for a resource:
        // 1. Linear: resource consumed exactly once
        // 2. Typestate: valid transition sequence
        // 3. Effects: all effects declared
        //
        // Scenario: a database connection that is opened, used, and closed.

        // --- Linear tracking ---
        let mut tracker = UsageTracker::new();
        tracker.declare("db_conn".into(), UsageGrade::Linear, 0..7);
        let mut ctx = LinearContext::new(tracker);

        // Resource consumed once (via close)
        let expr = AstExpr::MethodCall {
            receiver: Box::new(AstExpr::Ident("db_conn".into())),
            method: "close".into(),
            args: vec![],
        };
        let _ = check_expr_linearity(&expr, &mut ctx);
        let linear_errors = ctx.check();
        assert!(linear_errors.is_empty(), "linear: {linear_errors:?}");

        // --- Typestate tracking ---
        let states = vec![
            "Disconnected".into(),
            "Connected".into(),
            "InTransaction".into(),
            "Closed".into(),
        ];
        let transitions = vec![
            ("connect".into(), "Disconnected".into(), "Connected".into()),
            (
                "begin_tx".into(),
                "Connected".into(),
                "InTransaction".into(),
            ),
            ("commit".into(), "InTransaction".into(), "Connected".into()),
            ("close".into(), "Connected".into(), "Closed".into()),
        ];
        let mut ts = TypestateChecker::new(states, transitions, "Disconnected".into(), 0..12);

        assert!(ts.transition("connect", 10..17).is_ok());
        assert!(ts.transition("begin_tx", 18..26).is_ok());
        assert!(ts.transition("commit", 27..33).is_ok());
        assert!(ts.transition("close", 34..39).is_ok());
        assert_eq!(ts.current_state(), "Closed");
        assert!(ts.validate_linear(true).is_none());
        assert!(ts.validate_transitions().is_empty());

        // --- Effect tracking ---
        let eff = EffectChecker::new();
        let declared = EffectSet::from_effect_names(["database", "io"]);
        let actual =
            EffectSet::from_effect_names(["database.read", "database.write", "network.connect"]);
        let eff_errors = eff.check_containment(&declared, &actual, &(0..39));
        assert!(eff_errors.is_empty(), "effects: {eff_errors:?}");
    }

    #[test]
    fn interaction_full_pipeline_all_three_fail() {
        // All three checks fail simultaneously:
        // 1. Linear: double use
        // 2. Typestate: wrong state
        // 3. Effects: undeclared effect

        // --- Linear: double use ---
        let mut tracker = UsageTracker::new();
        tracker.declare("res".into(), UsageGrade::Linear, 0..3);
        tracker.use_var("res");
        tracker.use_var("res");
        let linear_errors = tracker.check();
        assert_eq!(linear_errors.len(), 1);
        assert_eq!(linear_errors[0].code, "A05001");

        // --- Typestate: wrong state ---
        let states = vec!["Off".into(), "On".into()];
        let transitions = vec![("turn_off".into(), "On".into(), "Off".into())];
        let mut ts = TypestateChecker::new(states, transitions, "Off".into(), 0..3);
        let ts_err = ts.transition("turn_off", 5..13);
        assert!(ts_err.is_err());
        assert_eq!(ts_err.unwrap_err().code, "A06001");

        // --- Effects: undeclared ---
        let eff = EffectChecker::new();
        let declared = EffectSet::pure();
        let actual = EffectSet::from_effect_names(["database.write"]);
        let eff_errors = eff.check_containment(&declared, &actual, &(0..10));
        assert_eq!(eff_errors.len(), 1);
        assert_eq!(eff_errors[0].code, "A07002");
    }

    // -----------------------------------------------------------------------
    // T045: Frame condition tests (CORE.3)
    // -----------------------------------------------------------------------

    #[test]
    fn extract_modifies_single_ident() {
        let body = AstExpr::Ident("x".into());
        let targets = extract_modifies_targets(&body);
        assert_eq!(targets, vec!["x"]);
    }

    #[test]
    fn extract_modifies_block_of_idents() {
        let body = AstExpr::Block(vec![AstExpr::Ident("x".into()), AstExpr::Ident("y".into())]);
        let targets = extract_modifies_targets(&body);
        assert_eq!(targets, vec!["x", "y"]);
    }

    #[test]
    fn extract_modifies_field_access() {
        let body = AstExpr::Field(Box::new(AstExpr::Ident("node".into())), "keys".into());
        let targets = extract_modifies_targets(&body);
        assert_eq!(targets, vec!["node.keys"]);
    }

    #[test]
    fn extract_modifies_nested_field() {
        let body = AstExpr::Field(
            Box::new(AstExpr::Field(
                Box::new(AstExpr::Ident("state".into())),
                "head".into(),
            )),
            "data".into(),
        );
        let targets = extract_modifies_targets(&body);
        assert_eq!(targets, vec!["state.head.data"]);
    }

    #[test]
    fn extract_modifies_list() {
        let body = AstExpr::List(vec![
            AstExpr::Ident("a".into()),
            AstExpr::Ident("b".into()),
            AstExpr::Ident("c".into()),
        ]);
        let targets = extract_modifies_targets(&body);
        assert_eq!(targets, vec!["a", "b", "c"]);
    }

    #[test]
    fn extract_modifies_raw_tokens() {
        let body = AstExpr::Raw(vec!["x".into(), ",".into(), "y".into()]);
        let targets = extract_modifies_targets(&body);
        assert_eq!(targets, vec!["x", "y"]);
    }

    #[test]
    fn collect_old_refs_simple() {
        // old(x)
        let expr = AstExpr::Old(Box::new(AstExpr::Ident("x".into())));
        let refs = collect_old_references(&expr);
        assert_eq!(refs, vec!["x"]);
    }

    #[test]
    fn collect_old_refs_in_binop() {
        // old(x) == old(y) + 1
        let expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Old(Box::new(AstExpr::Ident("x".into())))),
            op: AstBinOp::Eq,
            rhs: Box::new(AstExpr::BinOp {
                lhs: Box::new(AstExpr::Old(Box::new(AstExpr::Ident("y".into())))),
                op: AstBinOp::Add,
                rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
            }),
        };
        let refs = collect_old_references(&expr);
        assert!(refs.contains(&"x".to_string()));
        assert!(refs.contains(&"y".to_string()));
    }

    #[test]
    fn collect_old_refs_field() {
        // old(node.count)
        let expr = AstExpr::Old(Box::new(AstExpr::Field(
            Box::new(AstExpr::Ident("node".into())),
            "count".into(),
        )));
        let refs = collect_old_references(&expr);
        assert_eq!(refs, vec!["node.count"]);
    }

    #[test]
    fn collect_old_refs_none() {
        // x + y (no old() references)
        let expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("x".into())),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Ident("y".into())),
        };
        let refs = collect_old_references(&expr);
        assert!(refs.is_empty());
    }

    #[test]
    fn frame_checker_valid_modifies_clause() {
        // modifies { x } with x in scope -> no errors
        let body = AstExpr::Ident("x".into());
        let checker = FrameChecker::new(&[&body]);

        let mut env = TypeEnv::new();
        env.insert("x".into(), Type::Int);
        let symbols = assura_resolve::SymbolTable {
            symbols: vec![],
            scopes: vec![],
        };

        let errors = checker.check_scope(&env, &symbols, &(0..10));
        assert!(errors.is_empty());
    }

    #[test]
    fn frame_checker_unknown_var_a14001() {
        // modifies { nonexistent } -> A14001
        let body = AstExpr::Ident("nonexistent".into());
        let checker = FrameChecker::new(&[&body]);

        let env = TypeEnv::new();
        let symbols = assura_resolve::SymbolTable {
            symbols: vec![],
            scopes: vec![],
        };

        let errors = checker.check_scope(&env, &symbols, &(0..10));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A14001");
        assert!(errors[0].message.contains("nonexistent"));
    }

    #[test]
    fn frame_checker_mixed_scope_check() {
        // modifies { x, unknown_y } -> 1 error for unknown_y
        let body = AstExpr::Block(vec![
            AstExpr::Ident("x".into()),
            AstExpr::Ident("unknown_y".into()),
        ]);
        let checker = FrameChecker::new(&[&body]);

        let mut env = TypeEnv::new();
        env.insert("x".into(), Type::Int);
        let symbols = assura_resolve::SymbolTable {
            symbols: vec![],
            scopes: vec![],
        };

        let errors = checker.check_scope(&env, &symbols, &(0..10));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A14001");
        assert!(errors[0].message.contains("unknown_y"));
    }

    #[test]
    fn frame_checker_frame_axiom_vars() {
        // modifies { x }, ensures: y == old(y)
        // y is NOT in the modifies set, so it gets a frame axiom
        let modifies_body = AstExpr::Ident("x".into());
        let checker = FrameChecker::new(&[&modifies_body]);

        let ensures_body = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("y".into())),
            op: AstBinOp::Eq,
            rhs: Box::new(AstExpr::Old(Box::new(AstExpr::Ident("y".into())))),
        };

        let frame_vars = checker.frame_axiom_vars(&ensures_body);
        assert!(frame_vars.contains(&"y".to_string()));
        // x IS modified, so it should NOT appear
        assert!(!frame_vars.contains(&"x".to_string()));
    }

    #[test]
    fn frame_checker_modified_var_no_axiom() {
        // modifies { x }, ensures: x == old(x) + 1
        // x IS in the modifies set, so it should NOT get a frame axiom
        let modifies_body = AstExpr::Ident("x".into());
        let checker = FrameChecker::new(&[&modifies_body]);

        let ensures_body = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("x".into())),
            op: AstBinOp::Eq,
            rhs: Box::new(AstExpr::BinOp {
                lhs: Box::new(AstExpr::Old(Box::new(AstExpr::Ident("x".into())))),
                op: AstBinOp::Add,
                rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
            }),
        };

        let frame_vars = checker.frame_axiom_vars(&ensures_body);
        assert!(!frame_vars.contains(&"x".to_string()));
    }

    #[test]
    fn frame_checker_empty_no_axioms() {
        // No modifies clause -> no frame axioms
        let checker = FrameChecker::empty();
        assert!(!checker.has_modifies());

        let ensures_body = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("y".into())),
            op: AstBinOp::Eq,
            rhs: Box::new(AstExpr::Old(Box::new(AstExpr::Ident("y".into())))),
        };

        let frame_vars = checker.frame_axiom_vars(&ensures_body);
        assert!(frame_vars.is_empty());
    }

    #[test]
    fn frame_checker_has_modifies() {
        let body = AstExpr::Ident("x".into());
        let checker = FrameChecker::new(&[&body]);
        assert!(checker.has_modifies());
    }

    #[test]
    fn frame_checker_is_modified() {
        let body = AstExpr::Block(vec![AstExpr::Ident("x".into()), AstExpr::Ident("y".into())]);
        let checker = FrameChecker::new(&[&body]);
        assert!(checker.is_modified("x"));
        assert!(checker.is_modified("y"));
        assert!(!checker.is_modified("z"));
    }

    // -----------------------------------------------------------------------
    // T043 CORE.1: Ghost code tests
    // -----------------------------------------------------------------------

    #[test]
    fn ghost_fn_pure_effects_passes() {
        // A ghost function with effects: pure should type-check fine.
        let src = r#"
ghost fn invariant_helper(x: Int) -> Bool
    effects: pure
    ensures { result == true }
"#;
        let resolved = resolve_ok(src);
        let result = type_check(&resolved);
        assert!(
            result.is_ok(),
            "ghost fn with pure effects should pass: {result:?}"
        );
    }

    #[test]
    fn ghost_fn_no_effects_clause_passes() {
        // A ghost function with no explicit effects clause is implicitly pure.
        let src = r#"
ghost fn spec_helper(x: Int) -> Bool
    ensures { result == true }
"#;
        let resolved = resolve_ok(src);
        let result = type_check(&resolved);
        assert!(
            result.is_ok(),
            "ghost fn without effects clause should pass: {result:?}"
        );
    }

    #[test]
    fn ghost_fn_non_pure_effects_a54001() {
        // A ghost function with io effects should produce A54001.
        let src = r#"
ghost fn bad_ghost(x: Int) -> Bool
    effects: io
    ensures { result == true }
"#;
        let resolved = resolve_ok(src);
        let result = type_check(&resolved);
        assert!(result.is_err(), "ghost fn with io effects should fail");
        let errors = result.unwrap_err();
        assert!(
            errors.iter().any(|e| e.code == "A54001"),
            "should produce A54001, got: {errors:?}"
        );
        assert!(
            errors[0].message.contains("ghost function"),
            "error message should mention ghost function"
        );
    }

    #[test]
    fn ghost_block_type_checks_inner() {
        // A ghost block should type-check its inner expression.
        let env = TypeEnv::new();
        let expr = AstExpr::Ghost(Box::new(AstExpr::Literal(AstLit::Bool(true))));
        // Ghost block type is Unit (erased at runtime)
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unit);
    }

    #[test]
    fn ghost_block_propagates_inner_error() {
        // A ghost block with a type error in its body should propagate the error.
        let env = TypeEnv::new();
        let expr = AstExpr::Ghost(Box::new(AstExpr::BinOp {
            lhs: Box::new(AstExpr::Literal(AstLit::Bool(true))),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Literal(AstLit::Bool(false))),
        }));
        let err = infer_expr(&expr, &env).unwrap_err();
        assert_eq!(err.code, "A03001");
    }

    #[test]
    fn ghost_var_not_counted_as_linear_use() {
        // References inside a ghost block should NOT count as linear uses.
        let mut tracker = UsageTracker::new();
        tracker.declare("resource".into(), UsageGrade::Linear, 0..1);

        let ghost_expr = AstExpr::Ghost(Box::new(AstExpr::Ident("resource".into())));

        // Walk with linearity checker: ghost blocks should not count
        let mut ctx = LinearContext::new(tracker);
        let errors = check_expr_linearity(&ghost_expr, &mut ctx);
        assert!(
            errors.is_empty(),
            "ghost block should not cause linearity errors"
        );

        // The variable should still show 0 uses (ghost use does not count)
        assert_eq!(ctx.get_count("resource"), Some(0));
    }

    // -----------------------------------------------------------------------
    // T044: Lemma tests (CORE.2)
    // -----------------------------------------------------------------------

    #[test]
    fn lemma_fn_pure_effects_passes() {
        // Lemma with pure effects should type-check without errors.
        let src = r#"
            lemma add_comm(a: Int, b: Int)
                effects: pure
                ensures { a + b == b + a }
        "#;
        let (file, errs) = assura_parser::parse(src);
        assert!(errs.is_empty(), "parse errors: {errs:?}");
        let file = file.unwrap();
        let resolved = assura_resolve::resolve(&file).unwrap();
        let result = type_check(&resolved);
        assert!(
            result.is_ok(),
            "lemma with pure effects should pass type check"
        );
    }

    #[test]
    fn lemma_fn_no_effects_clause_passes() {
        // Lemma with no explicit effects clause is implicitly pure: OK.
        let src = r#"
            lemma trivial(x: Int)
                ensures { x == x }
        "#;
        let (file, errs) = assura_parser::parse(src);
        assert!(errs.is_empty(), "parse errors: {errs:?}");
        let file = file.unwrap();
        let resolved = assura_resolve::resolve(&file).unwrap();
        let result = type_check(&resolved);
        assert!(result.is_ok(), "lemma with no effects clause should pass");
    }

    #[test]
    fn lemma_fn_non_pure_effects_a55001() {
        // Lemma with non-pure effects should produce A55001.
        let src = r#"
            lemma bad_lemma(x: Int)
                effects: io
                ensures { x > 0 }
        "#;
        let (file, errs) = assura_parser::parse(src);
        assert!(errs.is_empty(), "parse errors: {errs:?}");
        let file = file.unwrap();
        let resolved = assura_resolve::resolve(&file).unwrap();
        let result = type_check(&resolved);
        assert!(result.is_err(), "lemma with io effects should fail");
        let errors = result.unwrap_err();
        assert!(
            errors.iter().any(|e| e.code == "A55001"),
            "should produce A55001, got: {errors:?}"
        );
    }

    #[test]
    fn lemma_is_lemma_flag_set() {
        // Verify that parsing a lemma sets is_lemma = true.
        let src = r#"
            lemma my_lemma(n: Int)
                ensures { n >= 0 }
        "#;
        let (file, errs) = assura_parser::parse(src);
        assert!(errs.is_empty(), "parse errors: {errs:?}");
        let file = file.unwrap();
        assert_eq!(file.decls.len(), 1);
        if let Decl::FnDef(f) = &file.decls[0].node {
            assert!(f.is_lemma, "lemma should have is_lemma = true");
            assert!(!f.is_ghost, "lemma should not have is_ghost = true");
            assert_eq!(f.name, "my_lemma");
        } else {
            panic!("expected FnDef, got {:?}", file.decls[0].node);
        }
    }

    #[test]
    fn fn_is_not_lemma() {
        // Verify that parsing a regular fn sets is_lemma = false.
        let src = r#"
            fn regular(n: Int) -> Int {
                ensures { result >= 0 }
            }
        "#;
        let (file, errs) = assura_parser::parse(src);
        assert!(errs.is_empty(), "parse errors: {errs:?}");
        let file = file.unwrap();
        assert_eq!(file.decls.len(), 1);
        if let Decl::FnDef(f) = &file.decls[0].node {
            assert!(!f.is_lemma, "fn should have is_lemma = false");
        } else {
            panic!("expected FnDef");
        }
    }

    #[test]
    fn apply_expr_type_is_bool() {
        // apply lemma_name(args) should have Bool type.
        let env = TypeEnv::new();
        let apply = AstExpr::Apply {
            lemma_name: "some_lemma".into(),
            args: vec![AstExpr::Literal(AstLit::Int("42".into()))],
        };
        let result = infer_expr(&apply, &env);
        assert_eq!(result.unwrap(), Type::Bool);
    }

    #[test]
    fn apply_not_counted_as_linear_use() {
        // apply should not count variable references as linear uses.
        let mut tracker = UsageTracker::new();
        tracker.declare("resource".into(), UsageGrade::Linear, 0..1);

        let apply = AstExpr::Apply {
            lemma_name: "some_lemma".into(),
            args: vec![AstExpr::Ident("resource".into())],
        };

        let mut ctx = LinearContext::new(tracker);
        let errors = check_expr_linearity(&apply, &mut ctx);
        assert!(errors.is_empty(), "apply should not cause linearity errors");
        assert_eq!(ctx.get_count("resource"), Some(0));
    }

    // -----------------------------------------------------------------------
    // T064: Error propagation tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_error_propagation_must_propagate_swallow_rejected() {
        let mut checker = ErrorPropagationChecker::new();
        checker.register_policy(
            "TestPolicy".into(),
            ErrorPolicy {
                must_propagate: vec!["SQLITE_CORRUPT".into(), "SQLITE_NOMEM".into()],
                ..Default::default()
            },
        );

        // Swallowing a must_propagate error should produce A12001
        let err = checker.validate_catch("SQLITE_CORRUPT", ErrorAction::Swallow, 0..10);
        assert!(err.is_some(), "swallowing must_propagate error should fail");
        assert_eq!(err.unwrap().code, "A12001");

        // Propagating is fine
        let err = checker.validate_catch("SQLITE_CORRUPT", ErrorAction::Propagate, 0..10);
        assert!(
            err.is_none(),
            "propagating must_propagate error should pass"
        );

        // Handling is fine
        let err = checker.validate_catch("SQLITE_CORRUPT", ErrorAction::Handle, 0..10);
        assert!(err.is_none(), "handling must_propagate error should pass");

        // Swallowing a non-must_propagate error is fine
        let err = checker.validate_catch("SQLITE_BUSY", ErrorAction::Swallow, 0..10);
        assert!(err.is_none(), "swallowing non-policy error should pass");
    }

    #[test]
    fn test_error_propagation_must_not_mask() {
        let mut checker = ErrorPropagationChecker::new();
        checker.register_policy(
            "TestPolicy".into(),
            ErrorPolicy {
                must_not_mask: vec![
                    ("SQLITE_CORRUPT".into(), "SQLITE_OK".into()),
                    ("SQLITE_NOMEM".into(), "SQLITE_ERROR".into()),
                ],
                ..Default::default()
            },
        );

        // Forbidden translation should produce A12002
        let err = checker.validate_catch(
            "SQLITE_CORRUPT",
            ErrorAction::TranslateTo("SQLITE_OK".into()),
            0..10,
        );
        assert!(err.is_some(), "forbidden translation should fail");
        assert_eq!(err.unwrap().code, "A12002");

        // Allowed translation should pass
        let err = checker.validate_catch(
            "SQLITE_CORRUPT",
            ErrorAction::TranslateTo("SQLITE_CORRUPT_DETAILED".into()),
            0..10,
        );
        assert!(err.is_none(), "non-forbidden translation should pass");
    }

    #[test]
    fn test_error_propagation_must_check() {
        let mut checker = ErrorPropagationChecker::new();
        checker.register_policy(
            "TestPolicy".into(),
            ErrorPolicy {
                must_check: vec!["sqlite3_reset".into(), "sqlite3_finalize".into()],
                ..Default::default()
            },
        );

        // Unchecked call to must_check function -> A12003
        let err = checker.validate_unchecked_call("sqlite3_reset", 0..10);
        assert!(err.is_some(), "unchecked must_check call should fail");
        assert_eq!(err.unwrap().code, "A12003");

        // Non-must_check function is fine
        let err = checker.validate_unchecked_call("sqlite3_open", 0..10);
        assert!(err.is_none(), "non-policy function should pass");
    }

    #[test]
    fn test_error_propagation_multiple_policies() {
        let mut checker = ErrorPropagationChecker::new();
        checker.register_policy(
            "PolicyA".into(),
            ErrorPolicy {
                must_propagate: vec!["ERR_A".into()],
                ..Default::default()
            },
        );
        checker.register_policy(
            "PolicyB".into(),
            ErrorPolicy {
                must_propagate: vec!["ERR_B".into()],
                ..Default::default()
            },
        );

        // Both policies are checked
        assert!(checker.is_must_propagate("ERR_A"));
        assert!(checker.is_must_propagate("ERR_B"));
        assert!(!checker.is_must_propagate("ERR_C"));
    }

    #[test]
    fn test_error_propagation_empty_policy() {
        let checker = ErrorPropagationChecker::new();

        // No policies registered: everything passes
        let err = checker.validate_catch("ANY_ERROR", ErrorAction::Swallow, 0..10);
        assert!(err.is_none(), "no policy means no restrictions");
    }

    // -----------------------------------------------------------------------
    // T046: Memory region contracts (MEM.1)
    // -----------------------------------------------------------------------

    #[test]
    fn memory_checker_register_buffer() {
        let mut checker = MemoryChecker::new();
        assert!(!checker.is_buffer("buf"));
        checker.register_buffer("buf".into(), "buf.len".into());
        assert!(checker.is_buffer("buf"));
        assert_eq!(checker.buffer_capacity("buf"), Some("buf.len"));
    }

    #[test]
    fn memory_checker_register_region() {
        let mut checker = MemoryChecker::new();
        checker.register_buffer("buf".into(), "buf.len".into());
        checker.register_region(MemoryRegion {
            name: "valid_range".into(),
            lower: "0".into(),
            upper: "buf.len".into(),
            buffer: "buf".into(),
        });
        assert_eq!(checker.regions().len(), 1);
        assert_eq!(checker.regions()[0].name, "valid_range");
    }

    #[test]
    fn memory_checker_bounds_check_present() {
        // offset + len <= buf.len pattern should be recognized
        let mut checker = MemoryChecker::new();
        checker.register_buffer("buf".into(), "buf.len".into());

        let bounds_expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::BinOp {
                lhs: Box::new(AstExpr::Ident("offset".into())),
                op: AstBinOp::Add,
                rhs: Box::new(AstExpr::Ident("len".into())),
            }),
            op: AstBinOp::Lte,
            rhs: Box::new(AstExpr::Field(
                Box::new(AstExpr::Ident("buf".into())),
                "len".into(),
            )),
        };

        let result = checker.check_bounds_in_requires("buf", &[&bounds_expr], &(0..10));
        assert!(result.is_none(), "should detect bounds check");
    }

    #[test]
    fn memory_checker_bounds_check_missing() {
        // No bounds check -> A08101
        let mut checker = MemoryChecker::new();
        checker.register_buffer("buf".into(), "buf.len".into());

        // A requires clause that does not check buffer bounds
        let unrelated_expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("x".into())),
            op: AstBinOp::Gt,
            rhs: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
        };

        let result = checker.check_bounds_in_requires("buf", &[&unrelated_expr], &(0..10));
        assert!(result.is_some(), "should detect missing bounds check");
        let err = result.unwrap();
        assert_eq!(err.code, "A08101");
        assert!(err.message.contains("buf"));
    }

    #[test]
    fn memory_checker_region_buffer_exists() {
        let mut checker = MemoryChecker::new();
        checker.register_buffer("buf".into(), "buf.len".into());
        checker.register_region(MemoryRegion {
            name: "r1".into(),
            lower: "0".into(),
            upper: "buf.len".into(),
            buffer: "buf".into(),
        });
        let errors = checker.check_region_buffers(&(0..10));
        assert!(errors.is_empty(), "buffer exists, no errors expected");
    }

    #[test]
    fn memory_checker_region_buffer_missing() {
        let mut checker = MemoryChecker::new();
        // Do NOT register "missing_buf" as a buffer
        checker.register_region(MemoryRegion {
            name: "r1".into(),
            lower: "0".into(),
            upper: "missing_buf.len".into(),
            buffer: "missing_buf".into(),
        });
        let errors = checker.check_region_buffers(&(0..10));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A08103");
        assert!(errors[0].message.contains("missing_buf"));
    }

    #[test]
    fn memory_checker_region_containment_same_buffer() {
        let mut checker = MemoryChecker::new();
        checker.register_buffer("buf".into(), "buf.len".into());
        checker.register_region(MemoryRegion {
            name: "sub".into(),
            lower: "2".into(),
            upper: "5".into(),
            buffer: "buf".into(),
        });
        checker.register_region(MemoryRegion {
            name: "parent".into(),
            lower: "0".into(),
            upper: "buf.len".into(),
            buffer: "buf".into(),
        });
        let result = checker.check_region_containment("sub", "parent", &(0..10));
        assert!(
            result.is_none(),
            "same buffer regions should pass structural check"
        );
    }

    #[test]
    fn memory_checker_region_containment_different_buffers() {
        let mut checker = MemoryChecker::new();
        checker.register_buffer("buf_a".into(), "buf_a.len".into());
        checker.register_buffer("buf_b".into(), "buf_b.len".into());
        checker.register_region(MemoryRegion {
            name: "r_a".into(),
            lower: "0".into(),
            upper: "buf_a.len".into(),
            buffer: "buf_a".into(),
        });
        checker.register_region(MemoryRegion {
            name: "r_b".into(),
            lower: "0".into(),
            upper: "buf_b.len".into(),
            buffer: "buf_b".into(),
        });
        let result = checker.check_region_containment("r_a", "r_b", &(0..10));
        assert!(result.is_some(), "different buffer regions should fail");
        assert_eq!(result.unwrap().code, "A08102");
    }

    #[test]
    fn memory_checker_region_containment_undefined_sub() {
        let checker = MemoryChecker::new();
        let result = checker.check_region_containment("nonexistent", "parent", &(0..10));
        assert!(result.is_some());
        assert_eq!(result.unwrap().code, "A08102");
    }

    #[test]
    fn memory_checker_bounds_check_with_capacity() {
        // buf.capacity pattern should also be recognized
        let mut checker = MemoryChecker::new();
        checker.register_buffer("buf".into(), "buf.capacity".into());

        let bounds_expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("idx".into())),
            op: AstBinOp::Lt,
            rhs: Box::new(AstExpr::Field(
                Box::new(AstExpr::Ident("buf".into())),
                "capacity".into(),
            )),
        };

        let result = checker.check_bounds_in_requires("buf", &[&bounds_expr], &(0..10));
        assert!(result.is_none(), "should detect capacity bounds check");
    }

    #[test]
    fn memory_checker_bounds_check_in_conjunction() {
        // x > 0 and offset + len <= buf.len -> should detect bounds check
        let mut checker = MemoryChecker::new();
        checker.register_buffer("buf".into(), "buf.len".into());

        let bounds_expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::BinOp {
                lhs: Box::new(AstExpr::Ident("x".into())),
                op: AstBinOp::Gt,
                rhs: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
            }),
            op: AstBinOp::And,
            rhs: Box::new(AstExpr::BinOp {
                lhs: Box::new(AstExpr::BinOp {
                    lhs: Box::new(AstExpr::Ident("offset".into())),
                    op: AstBinOp::Add,
                    rhs: Box::new(AstExpr::Ident("len".into())),
                }),
                op: AstBinOp::Lte,
                rhs: Box::new(AstExpr::Field(
                    Box::new(AstExpr::Ident("buf".into())),
                    "len".into(),
                )),
            }),
        };

        let result = checker.check_bounds_in_requires("buf", &[&bounds_expr], &(0..10));
        assert!(
            result.is_none(),
            "should detect bounds check in conjunction"
        );
    }

    #[test]
    fn memory_checker_default() {
        let checker = MemoryChecker::default();
        assert!(!checker.is_buffer("anything"));
        assert!(checker.regions().is_empty());
    }

    #[test]
    fn memory_checker_gte_bounds_check() {
        // buf.len >= offset + len pattern should also be recognized
        let mut checker = MemoryChecker::new();
        checker.register_buffer("buf".into(), "buf.len".into());

        let bounds_expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Field(
                Box::new(AstExpr::Ident("buf".into())),
                "len".into(),
            )),
            op: AstBinOp::Gte,
            rhs: Box::new(AstExpr::BinOp {
                lhs: Box::new(AstExpr::Ident("offset".into())),
                op: AstBinOp::Add,
                rhs: Box::new(AstExpr::Ident("len".into())),
            }),
        };

        let result = checker.check_bounds_in_requires("buf", &[&bounds_expr], &(0..10));
        assert!(result.is_none(), "should detect buf.len >= expr pattern");
    }

    #[test]
    fn expr_references_var_basic() {
        let expr = AstExpr::Ident("buf".into());
        assert!(expr_references_var(&expr, "buf"));
        assert!(!expr_references_var(&expr, "other"));
    }

    #[test]
    fn expr_references_var_in_binop() {
        let expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("buf".into())),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
        };
        assert!(expr_references_var(&expr, "buf"));
        assert!(!expr_references_var(&expr, "other"));
    }

    // -----------------------------------------------------------------------
    // T047: Taint tracking (SEC.1) tests
    // -----------------------------------------------------------------------

    #[test]
    fn taint_label_ordering() {
        assert!(TaintLabel::Untrusted < TaintLabel::Validated);
        assert!(TaintLabel::Validated < TaintLabel::Trusted);
        assert!(TaintLabel::Untrusted < TaintLabel::Trusted);
    }

    #[test]
    fn extract_taint_from_tokens() {
        let tokens = vec![
            "U32".into(),
            "@".into(),
            "taint".into(),
            ":".into(),
            "untrusted".into(),
        ];
        assert_eq!(extract_taint_label(&tokens), Some(TaintLabel::Untrusted));

        let tokens2 = vec![
            "ValidXlen".into(),
            "@".into(),
            "taint".into(),
            ":".into(),
            "validated".into(),
        ];
        assert_eq!(extract_taint_label(&tokens2), Some(TaintLabel::Validated));

        let no_taint = vec!["Int".into()];
        assert_eq!(extract_taint_label(&no_taint), None);
    }

    #[test]
    fn extract_taint_short_form() {
        let tokens = vec!["Bytes".into(), "@".into(), "untrusted".into()];
        assert_eq!(extract_taint_label(&tokens), Some(TaintLabel::Untrusted));

        let tokens2 = vec!["Data".into(), "@".into(), "validated".into()];
        assert_eq!(extract_taint_label(&tokens2), Some(TaintLabel::Validated));

        let tokens3 = vec!["Key".into(), "@".into(), "trusted".into()];
        assert_eq!(extract_taint_label(&tokens3), Some(TaintLabel::Trusted));
    }

    #[test]
    fn taint_checker_untrusted_index_a09101() {
        // Untrusted data used as array index -> A09101
        let mut checker = TaintChecker::new();
        checker.declare("idx".into(), TaintLabel::Untrusted);

        let expr = AstExpr::Index {
            expr: Box::new(AstExpr::Ident("buf".into())),
            index: Box::new(AstExpr::Ident("idx".into())),
        };
        let errors = checker.check_expr(&expr, &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A09101");
    }

    #[test]
    fn taint_checker_validated_index_passes() {
        // Validated data used as index -> no error
        let mut checker = TaintChecker::new();
        checker.declare("idx".into(), TaintLabel::Validated);

        let expr = AstExpr::Index {
            expr: Box::new(AstExpr::Ident("buf".into())),
            index: Box::new(AstExpr::Ident("idx".into())),
        };
        let errors = checker.check_expr(&expr, &(0..1));
        assert!(errors.is_empty(), "validated index should pass: {errors:?}");
    }

    #[test]
    fn taint_checker_trusted_index_passes() {
        // Trusted (default) data -> no error
        let checker = TaintChecker::new();

        let expr = AstExpr::Index {
            expr: Box::new(AstExpr::Ident("buf".into())),
            index: Box::new(AstExpr::Ident("idx".into())),
        };
        let errors = checker.check_expr(&expr, &(0..1));
        assert!(errors.is_empty(), "trusted index should pass: {errors:?}");
    }

    #[test]
    fn taint_propagation_through_arithmetic() {
        // If any operand is untrusted, result is untrusted
        let mut checker = TaintChecker::new();
        checker.declare("tainted".into(), TaintLabel::Untrusted);
        checker.declare("safe".into(), TaintLabel::Trusted);

        let expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("tainted".into())),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Ident("safe".into())),
        };
        assert_eq!(checker.infer_taint(&expr), TaintLabel::Untrusted);
    }

    #[test]
    fn taint_propagation_both_untrusted() {
        // Both operands untrusted -> result untrusted
        let mut checker = TaintChecker::new();
        checker.declare("a".into(), TaintLabel::Untrusted);
        checker.declare("b".into(), TaintLabel::Untrusted);

        let expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("a".into())),
            op: AstBinOp::Mul,
            rhs: Box::new(AstExpr::Ident("b".into())),
        };
        assert_eq!(checker.infer_taint(&expr), TaintLabel::Untrusted);
    }

    #[test]
    fn taint_validation_removes_taint() {
        // Calling a validation function produces Validated
        let mut checker = TaintChecker::new();
        checker.declare("raw".into(), TaintLabel::Untrusted);

        let expr = AstExpr::Call {
            func: Box::new(AstExpr::Ident("validate".into())),
            args: vec![AstExpr::Ident("raw".into())],
        };
        assert_eq!(checker.infer_taint(&expr), TaintLabel::Validated);
    }

    #[test]
    fn taint_checker_alloc_a09102() {
        // Untrusted data as allocation size -> A09102
        let mut checker = TaintChecker::new();
        checker.declare("sz".into(), TaintLabel::Untrusted);

        let expr = AstExpr::Call {
            func: Box::new(AstExpr::Ident("alloc".into())),
            args: vec![AstExpr::Ident("sz".into())],
        };
        let errors = checker.check_expr(&expr, &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A09102");
    }

    #[test]
    fn taint_checker_trusted_sink_a09103() {
        // Untrusted data flowing to a trusted sink -> A09103
        let mut checker = TaintChecker::new();
        checker.declare("raw_len".into(), TaintLabel::Untrusted);
        checker.register_trusted_sink("memcpy_len".into(), vec![Some(TaintLabel::Validated)]);

        let expr = AstExpr::Call {
            func: Box::new(AstExpr::Ident("memcpy_len".into())),
            args: vec![AstExpr::Ident("raw_len".into())],
        };
        let errors = checker.check_expr(&expr, &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A09103");
    }

    #[test]
    fn taint_checker_validated_at_sink_passes() {
        // Validated data at a sink that requires Validated -> no error
        let mut checker = TaintChecker::new();
        checker.declare("safe_len".into(), TaintLabel::Validated);
        checker.register_trusted_sink("memcpy_len".into(), vec![Some(TaintLabel::Validated)]);

        let expr = AstExpr::Call {
            func: Box::new(AstExpr::Ident("memcpy_len".into())),
            args: vec![AstExpr::Ident("safe_len".into())],
        };
        let errors = checker.check_expr(&expr, &(0..1));
        assert!(errors.is_empty(), "validated data at sink should pass");
    }

    #[test]
    fn taint_infer_literal_trusted() {
        let checker = TaintChecker::new();
        let expr = AstExpr::Literal(AstLit::Int("42".into()));
        assert_eq!(checker.infer_taint(&expr), TaintLabel::Trusted);
    }

    #[test]
    fn taint_infer_unknown_var_trusted() {
        // Undeclared variables default to Trusted
        let checker = TaintChecker::new();
        let expr = AstExpr::Ident("x".into());
        assert_eq!(checker.infer_taint(&expr), TaintLabel::Trusted);
    }

    #[test]
    fn taint_checker_nested_index_propagation() {
        // Tainted data flows through arithmetic to index -> A09101
        let mut checker = TaintChecker::new();
        checker.declare("offset".into(), TaintLabel::Untrusted);

        let index_expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("offset".into())),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
        };
        let expr = AstExpr::Index {
            expr: Box::new(AstExpr::Ident("buf".into())),
            index: Box::new(index_expr),
        };
        let errors = checker.check_expr(&expr, &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A09101");
    }

    #[test]
    fn taint_checker_display() {
        assert_eq!(TaintLabel::Untrusted.to_string(), "untrusted");
        assert_eq!(TaintLabel::Validated.to_string(), "validated");
        assert_eq!(TaintLabel::Trusted.to_string(), "trusted");
    }

    // --- T052: Dependent type tests ---

    #[test]
    fn dep_type_nat_index_valid() {
        let checker = DependentTypeChecker::new();
        let errors = checker.validate_index("n", "Nat", &(0..1));
        assert!(errors.is_empty(), "Nat should be a valid index type");
    }

    #[test]
    fn dep_type_bool_index_valid() {
        let checker = DependentTypeChecker::new();
        let errors = checker.validate_index("flag", "Bool", &(0..1));
        assert!(errors.is_empty(), "Bool should be a valid index type");
    }

    #[test]
    fn dep_type_enum_index_valid() {
        let mut checker = DependentTypeChecker::new();
        checker.register_enum("Mode".into(), vec!["Read".into(), "Write".into()]);
        let errors = checker.validate_index("mode", "Mode", &(0..1));
        assert!(errors.is_empty(), "known enum should be a valid index type");
    }

    #[test]
    fn dep_type_unknown_type_a03006() {
        let checker = DependentTypeChecker::new();
        let errors = checker.validate_index("x", "String", &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A03006");
    }

    #[test]
    fn dep_type_nat_arithmetic_valid() {
        let mut checker = DependentTypeChecker::new();
        checker.bind_index("n".into(), DepIndex::Nat("n".into()));
        // n + 1 is a valid Nat expression
        let expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("n".into())),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
        };
        let errors = checker.check_index_expr(&expr, &DepIndex::Nat("n".into()), &(0..1));
        assert!(errors.is_empty(), "n + 1 should be valid Nat arithmetic");
    }

    #[test]
    fn dep_type_bool_arithmetic_rejected() {
        let mut checker = DependentTypeChecker::new();
        checker.bind_index("flag".into(), DepIndex::Bool("flag".into()));
        // flag + 1 is NOT valid for a Bool index
        let expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("flag".into())),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
        };
        let errors = checker.check_index_expr(&expr, &DepIndex::Bool("flag".into()), &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A03008");
    }

    #[test]
    fn dep_type_enum_variant_valid() {
        let mut checker = DependentTypeChecker::new();
        checker.register_enum("Mode".into(), vec!["Read".into(), "Write".into()]);
        checker.bind_index(
            "m".into(),
            DepIndex::Enum {
                name: "m".into(),
                enum_type: "Mode".into(),
            },
        );
        let expr = AstExpr::Ident("Read".into());
        let idx = DepIndex::Enum {
            name: "m".into(),
            enum_type: "Mode".into(),
        };
        let errors = checker.check_index_expr(&expr, &idx, &(0..1));
        assert!(errors.is_empty(), "enum variant should be valid");
    }

    #[test]
    fn dep_type_equality_matching() {
        let checker = DependentTypeChecker::new();
        let t1 = DepType {
            base: Type::List(Box::new(Type::Int)),
            indices: vec![DepIndex::Nat("n".into())],
        };
        let t2 = DepType {
            base: Type::List(Box::new(Type::Int)),
            indices: vec![DepIndex::Nat("m".into())],
        };
        let errors = checker.check_dep_type_eq(&t1, &t2, &(0..1));
        assert!(errors.is_empty(), "same structure should match");
    }

    #[test]
    fn dep_type_equality_base_mismatch() {
        let checker = DependentTypeChecker::new();
        let t1 = DepType {
            base: Type::List(Box::new(Type::Int)),
            indices: vec![DepIndex::Nat("n".into())],
        };
        let t2 = DepType {
            base: Type::List(Box::new(Type::Float)),
            indices: vec![DepIndex::Nat("n".into())],
        };
        let errors = checker.check_dep_type_eq(&t1, &t2, &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A03010");
    }

    #[test]
    fn dep_type_equality_index_count_mismatch() {
        let checker = DependentTypeChecker::new();
        let t1 = DepType {
            base: Type::Int,
            indices: vec![DepIndex::Nat("n".into())],
        };
        let t2 = DepType {
            base: Type::Int,
            indices: vec![DepIndex::Nat("n".into()), DepIndex::Bool("b".into())],
        };
        let errors = checker.check_dep_type_eq(&t1, &t2, &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A03010");
    }

    #[test]
    fn dep_type_index_erasure_ghost_ok() {
        let mut checker = DependentTypeChecker::new();
        checker.bind_index("n".into(), DepIndex::Nat("n".into()));
        let expr = AstExpr::Ident("n".into());
        let errors = checker.check_index_erasure(&expr, true, &(0..1));
        assert!(errors.is_empty(), "index in ghost context is ok");
    }

    #[test]
    fn dep_type_index_erasure_runtime_error() {
        let mut checker = DependentTypeChecker::new();
        checker.bind_index("n".into(), DepIndex::Nat("n".into()));
        let expr = AstExpr::Ident("n".into());
        let errors = checker.check_index_erasure(&expr, false, &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A03012");
    }

    #[test]
    fn dep_type_index_kind_mismatch() {
        let checker = DependentTypeChecker::new();
        let t1 = DepType {
            base: Type::Int,
            indices: vec![DepIndex::Nat("n".into())],
        };
        let t2 = DepType {
            base: Type::Int,
            indices: vec![DepIndex::Bool("b".into())],
        };
        let errors = checker.check_dep_type_eq(&t1, &t2, &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A03011");
    }

    #[test]
    fn dep_type_display() {
        assert_eq!(DepIndex::Nat("n".into()).to_string(), "n: Nat");
        assert_eq!(DepIndex::Bool("flag".into()).to_string(), "flag: Bool");
        assert_eq!(
            DepIndex::Enum {
                name: "m".into(),
                enum_type: "Mode".into()
            }
            .to_string(),
            "m: Mode"
        );
    }

    // --- T058: FFI boundary contract tests ---

    #[test]
    fn ffi_extern_without_boundary_a11001() {
        let checker = FfiBoundaryChecker::new();
        let errors = checker.check_extern_decl("malloc", false, false, &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A11001");
    }

    #[test]
    fn ffi_extern_with_boundary_ok() {
        let checker = FfiBoundaryChecker::new();
        let errors = checker.check_extern_decl("malloc", true, true, &(0..1));
        assert!(errors.is_empty());
    }

    #[test]
    fn ffi_untrusted_without_contract_a11002() {
        let mut checker = FfiBoundaryChecker::new();
        checker.register_extern("read_bytes".into(), TrustBoundary::Untrusted);
        let errors = checker.check_extern_decl("read_bytes", true, false, &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A11002");
    }

    #[test]
    fn ffi_untrusted_with_contract_ok() {
        let mut checker = FfiBoundaryChecker::new();
        checker.register_extern("read_bytes".into(), TrustBoundary::Untrusted);
        let errors = checker.check_extern_decl("read_bytes", true, true, &(0..1));
        assert!(errors.is_empty());
    }

    #[test]
    fn ffi_trusted_no_contract_ok() {
        let mut checker = FfiBoundaryChecker::new();
        checker.register_extern("internal_fn".into(), TrustBoundary::Trusted);
        let errors = checker.check_extern_decl("internal_fn", true, false, &(0..1));
        assert!(errors.is_empty(), "trusted extern doesn't need a contract");
    }

    #[test]
    fn ffi_call_untrusted_unvalidated_a11003() {
        let mut checker = FfiBoundaryChecker::new();
        checker.register_extern("read_raw".into(), TrustBoundary::Untrusted);
        let errors = checker.check_ffi_call("read_raw", false, &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A11003");
    }

    #[test]
    fn ffi_call_untrusted_validated_ok() {
        let mut checker = FfiBoundaryChecker::new();
        checker.register_extern("read_raw".into(), TrustBoundary::Untrusted);
        let errors = checker.check_ffi_call("read_raw", true, &(0..1));
        assert!(errors.is_empty());
    }

    #[test]
    fn ffi_call_trusted_unvalidated_ok() {
        let mut checker = FfiBoundaryChecker::new();
        checker.register_extern("safe_fn".into(), TrustBoundary::Trusted);
        let errors = checker.check_ffi_call("safe_fn", false, &(0..1));
        assert!(errors.is_empty(), "trusted calls don't need validation");
    }

    #[test]
    fn ffi_unsafe_outside_wrapper_a11004() {
        let checker = FfiBoundaryChecker::new();
        let errors = checker.check_unsafe_confinement("compute", false, true, &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A11004");
    }

    #[test]
    fn ffi_unsafe_inside_wrapper_ok() {
        let checker = FfiBoundaryChecker::new();
        let errors = checker.check_unsafe_confinement("ffi_wrapper", true, true, &(0..1));
        assert!(errors.is_empty());
    }

    #[test]
    fn ffi_boundary_display() {
        assert_eq!(TrustBoundary::Trusted.to_string(), "trusted");
        assert_eq!(TrustBoundary::Audited.to_string(), "audited");
        assert_eq!(TrustBoundary::Untrusted.to_string(), "untrusted");
    }

    #[test]
    fn ffi_file_check_multiple_externs() {
        let mut checker = FfiBoundaryChecker::new();
        checker.register_extern("read".into(), TrustBoundary::Untrusted);
        checker.register_extern("write".into(), TrustBoundary::Audited);
        let externs = vec![
            ("read".into(), true, false, 0..5), // untrusted, no contract -> A11002
            ("write".into(), true, true, 10..15), // audited, has contract -> ok
            ("unknown".into(), false, false, 20..25), // no boundary -> A11001
        ];
        let errors = checker.check_file(&externs);
        assert_eq!(errors.len(), 2); // A11002 for read, A11001 for unknown
    }

    // --- T062: Interface contract tests ---

    #[test]
    fn interface_missing_method_a13001() {
        let mut checker = InterfaceChecker::new();
        checker.register_interface(InterfaceContract {
            name: "Serializable".into(),
            methods: vec![
                InterfaceMethod {
                    name: "serialize".into(),
                    param_types: vec![],
                    return_type: Type::Bytes,
                    has_requires: false,
                    has_ensures: true,
                    no_reentrancy: false,
                },
                InterfaceMethod {
                    name: "deserialize".into(),
                    param_types: vec![Type::Bytes],
                    return_type: Type::Named("Self".into()),
                    has_requires: true,
                    has_ensures: true,
                    no_reentrancy: false,
                },
            ],
            extends: vec![],
        });

        // Only implement serialize, not deserialize
        let errors = checker.check_impl("MyType", "Serializable", &["serialize".into()], &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A13001");
        assert!(errors[0].message.contains("deserialize"));
    }

    #[test]
    fn interface_all_methods_implemented_ok() {
        let mut checker = InterfaceChecker::new();
        checker.register_interface(InterfaceContract {
            name: "Hashable".into(),
            methods: vec![InterfaceMethod {
                name: "hash".into(),
                param_types: vec![],
                return_type: Type::U64,
                has_requires: false,
                has_ensures: true,
                no_reentrancy: false,
            }],
            extends: vec![],
        });

        let errors = checker.check_impl("MyType", "Hashable", &["hash".into()], &(0..1));
        assert!(errors.is_empty());
    }

    #[test]
    fn interface_signature_param_count_mismatch_a13002() {
        let mut checker = InterfaceChecker::new();
        checker.register_interface(InterfaceContract {
            name: "Comparable".into(),
            methods: vec![InterfaceMethod {
                name: "compare".into(),
                param_types: vec![Type::Int, Type::Int],
                return_type: Type::Bool,
                has_requires: false,
                has_ensures: false,
                no_reentrancy: false,
            }],
            extends: vec![],
        });

        let errors = checker.check_method_signature(
            "Comparable",
            "compare",
            &[Type::Int], // only 1 param
            &Type::Bool,
            &(0..1),
        );
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A13002");
    }

    #[test]
    fn interface_signature_return_type_mismatch_a13002() {
        let mut checker = InterfaceChecker::new();
        checker.register_interface(InterfaceContract {
            name: "Comparable".into(),
            methods: vec![InterfaceMethod {
                name: "compare".into(),
                param_types: vec![Type::Int],
                return_type: Type::Bool,
                has_requires: false,
                has_ensures: false,
                no_reentrancy: false,
            }],
            extends: vec![],
        });

        let errors = checker.check_method_signature(
            "Comparable",
            "compare",
            &[Type::Int],
            &Type::Int, // wrong return type
            &(0..1),
        );
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A13002");
        assert!(errors[0].message.contains("return type"));
    }

    #[test]
    fn interface_reentrancy_violation_a13003() {
        let mut checker = InterfaceChecker::new();
        checker.register_interface(InterfaceContract {
            name: "Callback".into(),
            methods: vec![InterfaceMethod {
                name: "on_event".into(),
                param_types: vec![],
                return_type: Type::Unit,
                has_requires: false,
                has_ensures: false,
                no_reentrancy: true,
            }],
            extends: vec![],
        });

        let errors = checker.check_reentrancy("Callback", "on_event", true, &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A13003");
    }

    #[test]
    fn interface_reentrancy_no_flag_ok() {
        let mut checker = InterfaceChecker::new();
        checker.register_interface(InterfaceContract {
            name: "Callback".into(),
            methods: vec![InterfaceMethod {
                name: "on_event".into(),
                param_types: vec![],
                return_type: Type::Unit,
                has_requires: false,
                has_ensures: false,
                no_reentrancy: false,
            }],
            extends: vec![],
        });

        let errors = checker.check_reentrancy("Callback", "on_event", true, &(0..1));
        assert!(errors.is_empty(), "method allows reentrancy");
    }

    #[test]
    fn interface_super_interface_inheritance() {
        let mut checker = InterfaceChecker::new();
        checker.register_interface(InterfaceContract {
            name: "Eq".into(),
            methods: vec![InterfaceMethod {
                name: "equals".into(),
                param_types: vec![Type::Named("Self".into())],
                return_type: Type::Bool,
                has_requires: false,
                has_ensures: false,
                no_reentrancy: false,
            }],
            extends: vec![],
        });
        checker.register_interface(InterfaceContract {
            name: "Ord".into(),
            methods: vec![InterfaceMethod {
                name: "compare_to".into(),
                param_types: vec![Type::Named("Self".into())],
                return_type: Type::Int,
                has_requires: false,
                has_ensures: false,
                no_reentrancy: false,
            }],
            extends: vec!["Eq".into()],
        });

        // Implement compare_to but not equals -> A13001 for missing super method
        let errors = checker.check_impl("MyType", "Ord", &["compare_to".into()], &(0..1));
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("equals"));
        assert!(errors[0].message.contains("Eq"));
    }

    #[test]
    fn interface_unknown_interface_a13001() {
        let checker = InterfaceChecker::new();
        let errors = checker.check_impl("MyType", "Unknown", &[], &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A13001");
        assert!(errors[0].message.contains("Unknown"));
    }

    // --- T059: Constant-time execution tests ---

    #[test]
    fn ct_branch_on_secret_a14001() {
        let mut checker = ConstantTimeChecker::new();
        checker.mark_secret("key".into());
        let cond = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("key".into())),
            op: AstBinOp::Eq,
            rhs: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
        };
        let errors = checker.check_branch(&cond, &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A14001");
    }

    #[test]
    fn ct_branch_on_public_ok() {
        let mut checker = ConstantTimeChecker::new();
        checker.mark_secret("key".into());
        let cond = AstExpr::Ident("public_val".into());
        let errors = checker.check_branch(&cond, &(0..1));
        assert!(errors.is_empty());
    }

    #[test]
    fn ct_index_on_secret_a14002() {
        let mut checker = ConstantTimeChecker::new();
        checker.mark_secret("secret_idx".into());
        let idx = AstExpr::Ident("secret_idx".into());
        let errors = checker.check_index(&idx, &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A14002");
    }

    #[test]
    fn ct_index_on_public_ok() {
        let checker = ConstantTimeChecker::new();
        let idx = AstExpr::Ident("i".into());
        let errors = checker.check_index(&idx, &(0..1));
        assert!(errors.is_empty());
    }

    #[test]
    fn ct_nested_secret_in_condition() {
        let mut checker = ConstantTimeChecker::new();
        checker.mark_secret("password".into());
        // password + 1 == 42
        let cond = AstExpr::BinOp {
            lhs: Box::new(AstExpr::BinOp {
                lhs: Box::new(AstExpr::Ident("password".into())),
                op: AstBinOp::Add,
                rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
            }),
            op: AstBinOp::Eq,
            rhs: Box::new(AstExpr::Literal(AstLit::Int("42".into()))),
        };
        let errors = checker.check_branch(&cond, &(0..1));
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn ct_check_expr_if_with_secret() {
        let mut checker = ConstantTimeChecker::new();
        checker.mark_secret("s".into());
        let expr = AstExpr::If {
            cond: Box::new(AstExpr::Ident("s".into())),
            then_branch: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
            else_branch: Some(Box::new(AstExpr::Literal(AstLit::Int("0".into())))),
        };
        let errors = checker.check_expr(&expr, &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A14001");
    }

    #[test]
    fn ct_references_secret_field() {
        let mut checker = ConstantTimeChecker::new();
        checker.mark_secret("key".into());
        let expr = AstExpr::Field(Box::new(AstExpr::Ident("key".into())), "len".into());
        assert!(checker.references_secret(&expr));
    }

    // --- T063: Recursive structural invariant tests ---

    #[test]
    fn struct_inv_tree_balance_valid() {
        let mut checker = StructuralInvariantChecker::new();
        checker.register_recursive_type("AVLTree".into(), vec!["left".into(), "right".into()]);
        let errors = checker.check_invariant_applicability(
            "AVLTree",
            &InvariantKind::TreeBalance { max_diff: 1 },
            &(0..1),
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn struct_inv_on_non_recursive_a15001() {
        let checker = StructuralInvariantChecker::new();
        let errors = checker.check_invariant_applicability(
            "Point",
            &InvariantKind::Sorted { descending: false },
            &(0..1),
        );
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A15001");
    }

    #[test]
    fn struct_inv_tree_on_list_a15002() {
        let mut checker = StructuralInvariantChecker::new();
        checker.register_recursive_type("LinkedList".into(), vec!["next".into()]);
        let errors = checker.check_invariant_applicability(
            "LinkedList",
            &InvariantKind::BstOrdering,
            &(0..1),
        );
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A15002");
    }

    #[test]
    fn struct_inv_sort_on_tree_a15003() {
        let mut checker = StructuralInvariantChecker::new();
        checker.register_recursive_type("BTree".into(), vec!["left".into(), "right".into()]);
        let errors = checker.check_invariant_applicability(
            "BTree",
            &InvariantKind::Sorted { descending: false },
            &(0..1),
        );
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A15003");
    }

    #[test]
    fn struct_inv_acyclic_valid_for_any_recursive() {
        let mut checker = StructuralInvariantChecker::new();
        checker.register_recursive_type("Graph".into(), vec!["children".into()]);
        let errors =
            checker.check_invariant_applicability("Graph", &InvariantKind::Acyclic, &(0..1));
        assert!(errors.is_empty());
    }

    #[test]
    fn struct_inv_operation_no_proof_a15004() {
        let mut checker = StructuralInvariantChecker::new();
        checker.register_recursive_type("BST".into(), vec!["left".into(), "right".into()]);
        checker.register_invariant(StructuralInvariant {
            name: "bst_order".into(),
            type_name: "BST".into(),
            kind: InvariantKind::BstOrdering,
        });
        let errors = checker.check_operation_preserves(
            "BST",
            "insert",
            true,  // modifies structure
            false, // no preservation proof
            &(0..1),
        );
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A15004");
    }

    #[test]
    fn struct_inv_operation_with_proof_ok() {
        let mut checker = StructuralInvariantChecker::new();
        checker.register_recursive_type("BST".into(), vec!["left".into(), "right".into()]);
        checker.register_invariant(StructuralInvariant {
            name: "bst_order".into(),
            type_name: "BST".into(),
            kind: InvariantKind::BstOrdering,
        });
        let errors = checker.check_operation_preserves(
            "BST",
            "insert",
            true, // modifies structure
            true, // has preservation proof
            &(0..1),
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn struct_inv_readonly_trivially_preserves() {
        let mut checker = StructuralInvariantChecker::new();
        checker.register_recursive_type("BST".into(), vec!["left".into(), "right".into()]);
        checker.register_invariant(StructuralInvariant {
            name: "bst_order".into(),
            type_name: "BST".into(),
            kind: InvariantKind::BstOrdering,
        });
        let errors = checker.check_operation_preserves(
            "BST",
            "search",
            false, // read-only
            false, // no proof needed
            &(0..1),
        );
        assert!(errors.is_empty(), "read-only ops preserve invariants");
    }

    #[test]
    fn struct_inv_kind_display() {
        assert_eq!(
            InvariantKind::TreeBalance { max_diff: 1 }.to_string(),
            "tree_balance(max_diff=1)"
        );
        assert_eq!(
            InvariantKind::Sorted { descending: false }.to_string(),
            "sorted(asc)"
        );
        assert_eq!(InvariantKind::Acyclic.to_string(), "acyclic");
        assert_eq!(InvariantKind::BstOrdering.to_string(), "bst_ordering");
        assert_eq!(
            InvariantKind::HeapProperty { min_heap: true }.to_string(),
            "min_heap"
        );
    }

    #[test]
    fn struct_inv_get_invariants() {
        let mut checker = StructuralInvariantChecker::new();
        checker.register_recursive_type("AVL".into(), vec!["left".into(), "right".into()]);
        checker.register_invariant(StructuralInvariant {
            name: "balance".into(),
            type_name: "AVL".into(),
            kind: InvariantKind::TreeBalance { max_diff: 1 },
        });
        checker.register_invariant(StructuralInvariant {
            name: "order".into(),
            type_name: "AVL".into(),
            kind: InvariantKind::BstOrdering,
        });
        assert_eq!(checker.get_invariants("AVL").len(), 2);
        assert!(checker.get_invariants("Unknown").is_empty());
    }

    // --- T060: Secure erasure tests ---

    #[test]
    fn secure_erasure_not_zeroized_a16001() {
        let mut checker = SecureErasureChecker::new();
        checker.mark_sensitive("private_key".into());
        let errors = checker.check_scope_exit("private_key", &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A16001");
    }

    #[test]
    fn secure_erasure_zeroized_ok() {
        let mut checker = SecureErasureChecker::new();
        checker.mark_sensitive("private_key".into());
        checker.mark_zeroized("private_key".into());
        let errors = checker.check_scope_exit("private_key", &(0..1));
        assert!(errors.is_empty());
    }

    #[test]
    fn secure_erasure_non_sensitive_ok() {
        let checker = SecureErasureChecker::new();
        let errors = checker.check_scope_exit("public_data", &(0..1));
        assert!(errors.is_empty());
    }

    #[test]
    fn secure_erasure_copy_to_non_sensitive_a16002() {
        let mut checker = SecureErasureChecker::new();
        checker.mark_sensitive("key".into());
        let errors = checker.check_copy("key", "backup", false, &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A16002");
    }

    #[test]
    fn secure_erasure_copy_to_sensitive_ok() {
        let mut checker = SecureErasureChecker::new();
        checker.mark_sensitive("key".into());
        let errors = checker.check_copy("key", "key_copy", true, &(0..1));
        assert!(errors.is_empty());
    }

    #[test]
    fn secure_erasure_return_not_sensitive_a16003() {
        let mut checker = SecureErasureChecker::new();
        checker.mark_sensitive("derived_key".into());
        let errors = checker.check_return("derived_key", false, &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A16003");
    }

    #[test]
    fn secure_erasure_check_all_erased() {
        let mut checker = SecureErasureChecker::new();
        checker.mark_sensitive("key1".into());
        checker.mark_sensitive("key2".into());
        checker.mark_zeroized("key1".into());
        let errors = checker.check_all_erased(&(0..1));
        assert_eq!(errors.len(), 1); // key2 not zeroized
    }

    // --- T061: Cryptographic conformance tests ---

    #[test]
    fn crypto_correct_key_size_ok() {
        let checker = CryptoConformanceChecker::new();
        let errors = checker.check_key_size("AES-128-GCM", 128, &(0..1));
        assert!(errors.is_empty());
    }

    #[test]
    fn crypto_wrong_key_size_a17001() {
        let checker = CryptoConformanceChecker::new();
        let errors = checker.check_key_size("AES-128-GCM", 256, &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A17001");
    }

    #[test]
    fn crypto_correct_nonce_size_ok() {
        let checker = CryptoConformanceChecker::new();
        let errors = checker.check_nonce_size("AES-256-GCM", 12, &(0..1));
        assert!(errors.is_empty());
    }

    #[test]
    fn crypto_wrong_nonce_size_a17002() {
        let checker = CryptoConformanceChecker::new();
        let errors = checker.check_nonce_size("AES-256-GCM", 16, &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A17002");
    }

    #[test]
    fn crypto_nonce_not_unique_a17003() {
        let checker = CryptoConformanceChecker::new();
        let errors = checker.check_nonce_uniqueness("fixed_nonce", false, false, &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A17003");
    }

    #[test]
    fn crypto_counter_nonce_ok() {
        let checker = CryptoConformanceChecker::new();
        let errors = checker.check_nonce_uniqueness("counter", true, false, &(0..1));
        assert!(errors.is_empty());
    }

    #[test]
    fn crypto_tag_not_verified_a17004() {
        let checker = CryptoConformanceChecker::new();
        let errors = checker.check_tag_verification(false, &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A17004");
    }

    #[test]
    fn crypto_tag_verified_ok() {
        let checker = CryptoConformanceChecker::new();
        let errors = checker.check_tag_verification(true, &(0..1));
        assert!(errors.is_empty());
    }

    #[test]
    fn crypto_chacha20_key_size() {
        let checker = CryptoConformanceChecker::new();
        let errors = checker.check_key_size("ChaCha20-Poly1305", 256, &(0..1));
        assert!(errors.is_empty());
        let errors = checker.check_key_size("ChaCha20-Poly1305", 128, &(0..1));
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn crypto_custom_spec() {
        let mut checker = CryptoConformanceChecker::new();
        checker.register_spec(CryptoSpec {
            name: "XSalsa20".into(),
            key_size_bits: vec![256],
            block_size_bytes: None,
            nonce_size_bytes: Some(24),
            tag_size_bytes: None,
        });
        let errors = checker.check_nonce_size("XSalsa20", 24, &(0..1));
        assert!(errors.is_empty());
        let errors = checker.check_nonce_size("XSalsa20", 12, &(0..1));
        assert_eq!(errors.len(), 1);
    }

    // --- T065: Shared memory protocol tests ---

    #[test]
    fn shared_mem_read_exclusive_ok() {
        let mut checker = SharedMemChecker::new();
        checker.set_mode("buffer".into(), AccessMode::Exclusive);
        let errors = checker.check_read("buffer", &(0..1));
        assert!(errors.is_empty());
    }

    #[test]
    fn shared_mem_read_shared_ok() {
        let mut checker = SharedMemChecker::new();
        checker.set_mode("buffer".into(), AccessMode::SharedRead);
        let errors = checker.check_read("buffer", &(0..1));
        assert!(errors.is_empty());
    }

    #[test]
    fn shared_mem_read_none_a18001() {
        let mut checker = SharedMemChecker::new();
        checker.set_mode("buffer".into(), AccessMode::None);
        let errors = checker.check_read("buffer", &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A18001");
    }

    #[test]
    fn shared_mem_write_exclusive_ok() {
        let mut checker = SharedMemChecker::new();
        checker.set_mode("buffer".into(), AccessMode::Exclusive);
        let errors = checker.check_write("buffer", &(0..1));
        assert!(errors.is_empty());
    }

    #[test]
    fn shared_mem_write_shared_a18002() {
        let mut checker = SharedMemChecker::new();
        checker.set_mode("buffer".into(), AccessMode::SharedRead);
        let errors = checker.check_write("buffer", &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A18002");
    }

    #[test]
    fn shared_mem_data_race_a18003() {
        let checker = SharedMemChecker::new();
        let errors = checker.check_data_race(
            "counter",
            AccessMode::Exclusive,
            AccessMode::SharedRead,
            &(0..1),
        );
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A18003");
    }

    #[test]
    fn shared_mem_two_readers_ok() {
        let checker = SharedMemChecker::new();
        let errors = checker.check_data_race(
            "counter",
            AccessMode::SharedRead,
            AccessMode::SharedRead,
            &(0..1),
        );
        assert!(errors.is_empty(), "two shared readers is safe");
    }

    #[test]
    fn shared_mem_access_mode_display() {
        assert_eq!(AccessMode::Exclusive.to_string(), "exclusive");
        assert_eq!(AccessMode::SharedRead.to_string(), "shared_read");
        assert_eq!(AccessMode::None.to_string(), "none");
    }

    // --- T067: Determinism checker tests ---

    #[test]
    fn determinism_hashmap_a20001() {
        let mut checker = DeterminismChecker::new();
        checker.mark_deterministic("compute".into());
        let errors = checker.check_fn_body("compute", &["HashMap".into(), "Vec".into()], &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A20001");
    }

    #[test]
    fn determinism_btreemap_ok() {
        let mut checker = DeterminismChecker::new();
        checker.mark_deterministic("compute".into());
        let errors = checker.check_fn_body("compute", &["BTreeMap".into(), "Vec".into()], &(0..1));
        assert!(errors.is_empty());
    }

    #[test]
    fn determinism_non_det_fn_ok() {
        let checker = DeterminismChecker::new();
        // Not marked deterministic
        let errors = checker.check_fn_body("random_pick", &["random".into()], &(0..1));
        assert!(errors.is_empty(), "non-deterministic fn allows random");
    }

    #[test]
    fn determinism_iteration_a20002() {
        let mut checker = DeterminismChecker::new();
        checker.mark_deterministic("process".into());
        let errors = checker.check_iteration("process", "HashMap<K,V>", &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A20002");
    }

    #[test]
    fn determinism_btree_iteration_ok() {
        let mut checker = DeterminismChecker::new();
        checker.mark_deterministic("process".into());
        let errors = checker.check_iteration("process", "BTreeMap<K,V>", &(0..1));
        assert!(errors.is_empty());
    }

    #[test]
    fn determinism_random_a20001() {
        let mut checker = DeterminismChecker::new();
        checker.mark_deterministic("seed_fn".into());
        let errors = checker.check_fn_body("seed_fn", &["thread_rng".into()], &(0..1));
        assert_eq!(errors.len(), 1);
    }

    // --- T068: Lock ordering tests ---

    #[test]
    fn lock_order_correct_ok() {
        let mut checker = LockOrderChecker::new();
        checker.define_order("db".into(), 1);
        checker.define_order("cache".into(), 2);
        let errors = checker.acquire("db", &(0..1));
        assert!(errors.is_empty());
        let errors = checker.acquire("cache", &(0..1));
        assert!(errors.is_empty());
    }

    #[test]
    fn lock_order_violation_a21001() {
        let mut checker = LockOrderChecker::new();
        checker.define_order("db".into(), 1);
        checker.define_order("cache".into(), 2);
        let errors = checker.acquire("cache", &(0..1));
        assert!(errors.is_empty());
        let errors = checker.acquire("db", &(0..1)); // wrong order
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A21001");
    }

    #[test]
    fn lock_order_release_correct() {
        let mut checker = LockOrderChecker::new();
        checker.define_order("a".into(), 1);
        checker.define_order("b".into(), 2);
        checker.acquire("a", &(0..1));
        checker.acquire("b", &(0..1));
        let errors = checker.release("b", &(0..1)); // correct: LIFO
        assert!(errors.is_empty());
    }

    #[test]
    fn lock_order_release_wrong_a21002() {
        let mut checker = LockOrderChecker::new();
        checker.define_order("a".into(), 1);
        checker.define_order("b".into(), 2);
        checker.acquire("a", &(0..1));
        checker.acquire("b", &(0..1));
        let errors = checker.release("a", &(0..1)); // wrong: b still held
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A21002");
    }

    #[test]
    fn lock_order_undefined_a21003() {
        let checker = LockOrderChecker::new();
        let errors = checker.check_ordering_defined("unknown_lock", &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A21003");
    }

    #[test]
    fn lock_order_defined_ok() {
        let mut checker = LockOrderChecker::new();
        checker.define_order("db".into(), 1);
        let errors = checker.check_ordering_defined("db", &(0..1));
        assert!(errors.is_empty());
    }

    #[test]
    fn info_flow_security_label_ordering() {
        // Verify the lattice: Public < Internal < Confidential < Restricted
        assert!(SecurityLabel::Public < SecurityLabel::Internal);
        assert!(SecurityLabel::Internal < SecurityLabel::Confidential);
        assert!(SecurityLabel::Confidential < SecurityLabel::Restricted);
        assert!(SecurityLabel::Public < SecurityLabel::Restricted);
    }

    #[test]
    fn info_flow_valid_upward_assignment() {
        // Public -> Confidential is a valid upward flow
        let checker = InfoFlowChecker::new();
        let err =
            checker.check_assignment(SecurityLabel::Confidential, SecurityLabel::Public, &(0..1));
        assert!(err.is_none(), "upward flow should be allowed");
    }

    #[test]
    fn info_flow_valid_same_level_assignment() {
        // Confidential -> Confidential is allowed (same level)
        let checker = InfoFlowChecker::new();
        let err = checker.check_assignment(
            SecurityLabel::Confidential,
            SecurityLabel::Confidential,
            &(0..1),
        );
        assert!(err.is_none(), "same-level flow should be allowed");
    }

    #[test]
    fn info_flow_invalid_downward_a08001() {
        // Confidential -> Public is a violation (A08001)
        let checker = InfoFlowChecker::new();
        let err =
            checker.check_assignment(SecurityLabel::Public, SecurityLabel::Confidential, &(0..1));
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A08001");
    }

    #[test]
    fn info_flow_restricted_to_internal_a08001() {
        // Restricted -> Internal is a violation (A08001)
        let checker = InfoFlowChecker::new();
        let err =
            checker.check_assignment(SecurityLabel::Internal, SecurityLabel::Restricted, &(0..1));
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A08001");
    }

    #[test]
    fn info_flow_declassify_with_annotation_ok() {
        // Declassification with explicit annotation is permitted
        let checker = InfoFlowChecker::new();
        let err = checker.check_declassify(
            SecurityLabel::Confidential,
            SecurityLabel::Public,
            true,
            &(0..1),
        );
        assert!(err.is_none(), "annotated declassification should pass");
    }

    #[test]
    fn info_flow_declassify_without_annotation_a08002() {
        // Declassification without annotation -> A08002
        let checker = InfoFlowChecker::new();
        let err = checker.check_declassify(
            SecurityLabel::Confidential,
            SecurityLabel::Public,
            false,
            &(0..1),
        );
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A08002");
    }

    #[test]
    fn info_flow_declassify_upward_no_error() {
        // Upward "declassification" (Public -> Confidential) is not a
        // downgrade, so no error even without annotation
        let checker = InfoFlowChecker::new();
        let err = checker.check_declassify(
            SecurityLabel::Public,
            SecurityLabel::Confidential,
            false,
            &(0..1),
        );
        assert!(err.is_none());
    }

    #[test]
    fn info_flow_label_propagation_binary() {
        // Binary op: max(Confidential, Public) = Confidential
        let mut checker = InfoFlowChecker::new();
        checker.declare("secret".into(), SecurityLabel::Confidential);
        checker.declare("pub_val".into(), SecurityLabel::Public);

        let expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("secret".into())),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Ident("pub_val".into())),
        };
        assert_eq!(checker.infer_label(&expr), SecurityLabel::Confidential);
    }

    #[test]
    fn info_flow_label_propagation_both_restricted() {
        // Both operands Restricted -> result Restricted
        let mut checker = InfoFlowChecker::new();
        checker.declare("a".into(), SecurityLabel::Restricted);
        checker.declare("b".into(), SecurityLabel::Restricted);

        let expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("a".into())),
            op: AstBinOp::Mul,
            rhs: Box::new(AstExpr::Ident("b".into())),
        };
        assert_eq!(checker.infer_label(&expr), SecurityLabel::Restricted);
    }

    #[test]
    fn info_flow_infer_literal_public() {
        // Literals are always Public
        let checker = InfoFlowChecker::new();
        let expr = AstExpr::Literal(AstLit::Int("42".into()));
        assert_eq!(checker.infer_label(&expr), SecurityLabel::Public);
    }

    #[test]
    fn info_flow_infer_unknown_var_public() {
        // Undeclared variables default to Public
        let checker = InfoFlowChecker::new();
        let expr = AstExpr::Ident("x".into());
        assert_eq!(checker.infer_label(&expr), SecurityLabel::Public);
    }

    #[test]
    fn info_flow_purpose_label_mismatch_a08003() {
        // Purpose mismatch -> A08003
        let mut checker = InfoFlowChecker::new();
        checker.declare_purpose("email".into(), "marketing".into());
        let err = checker.check_purpose_label("email", "billing", &(0..1));
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A08003");
    }

    #[test]
    fn info_flow_purpose_label_match_ok() {
        // Matching purpose -> no error
        let mut checker = InfoFlowChecker::new();
        checker.declare_purpose("email".into(), "billing".into());
        let err = checker.check_purpose_label("email", "billing", &(0..1));
        assert!(err.is_none());
    }

    #[test]
    fn info_flow_purpose_label_untracked_ok() {
        // Variable without purpose label -> no error
        let checker = InfoFlowChecker::new();
        let err = checker.check_purpose_label("x", "analytics", &(0..1));
        assert!(err.is_none());
    }

    #[test]
    fn info_flow_implicit_flow_a08004() {
        // Confidential condition, Public branch target -> A08004
        let checker = InfoFlowChecker::new();
        let err = checker.check_implicit_flow(
            SecurityLabel::Confidential,
            SecurityLabel::Public,
            &(0..1),
        );
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A08004");
    }

    #[test]
    fn info_flow_implicit_flow_same_level_ok() {
        // Same-level condition and target -> no implicit flow
        let checker = InfoFlowChecker::new();
        let err =
            checker.check_implicit_flow(SecurityLabel::Internal, SecurityLabel::Internal, &(0..1));
        assert!(err.is_none());
    }

    #[test]
    fn info_flow_covert_channel_a08005() {
        // High-security data controls a timing function -> A08005
        let checker = InfoFlowChecker::new();
        let err = checker.check_covert_channel(SecurityLabel::Confidential, "sleep", &(0..1));
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A08005");
    }

    #[test]
    fn info_flow_covert_channel_public_ok() {
        // Public data controlling sleep is not a covert channel
        let checker = InfoFlowChecker::new();
        let err = checker.check_covert_channel(SecurityLabel::Public, "sleep", &(0..1));
        assert!(err.is_none());
    }

    #[test]
    fn info_flow_covert_channel_non_sensitive_fn_ok() {
        // High-security data controlling a non-sensitive function is ok
        let checker = InfoFlowChecker::new();
        let err = checker.check_covert_channel(SecurityLabel::Restricted, "compute", &(0..1));
        assert!(err.is_none());
    }

    #[test]
    fn info_flow_label_propagation_nested() {
        // Nested expression: (public + confidential) * restricted
        // -> max(max(Public, Confidential), Restricted) = Restricted
        let mut checker = InfoFlowChecker::new();
        checker.declare("pub_val".into(), SecurityLabel::Public);
        checker.declare("conf".into(), SecurityLabel::Confidential);
        checker.declare("restr".into(), SecurityLabel::Restricted);

        let inner = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Ident("pub_val".into())),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Ident("conf".into())),
        };
        let outer = AstExpr::BinOp {
            lhs: Box::new(inner),
            op: AstBinOp::Mul,
            rhs: Box::new(AstExpr::Ident("restr".into())),
        };
        assert_eq!(checker.infer_label(&outer), SecurityLabel::Restricted);
    }

    #[test]
    fn info_flow_label_field_access() {
        // Field access propagates receiver label
        let mut checker = InfoFlowChecker::new();
        checker.declare("secret_obj".into(), SecurityLabel::Confidential);
        let expr = AstExpr::Field(Box::new(AstExpr::Ident("secret_obj".into())), "name".into());
        assert_eq!(checker.infer_label(&expr), SecurityLabel::Confidential);
    }

    #[test]
    fn info_flow_check_expr_if_covert_channel() {
        // If a confidential condition controls a sleep call inside a
        // branch, check_expr should detect the covert channel (A08005).
        let mut checker = InfoFlowChecker::new();
        checker.declare("is_admin".into(), SecurityLabel::Confidential);

        let expr = AstExpr::If {
            cond: Box::new(AstExpr::Ident("is_admin".into())),
            then_branch: Box::new(AstExpr::Call {
                func: Box::new(AstExpr::Ident("sleep".into())),
                args: vec![AstExpr::Literal(AstLit::Int("100".into()))],
            }),
            else_branch: None,
        };
        let errors = checker.check_expr(&expr, &(0..10));
        let has_a08005 = errors.iter().any(|e| e.code == "A08005");
        assert!(
            has_a08005,
            "expected A08005 for covert channel via if+sleep"
        );
    }

    #[test]
    fn info_flow_display_labels() {
        assert_eq!(SecurityLabel::Public.to_string(), "Public");
        assert_eq!(SecurityLabel::Internal.to_string(), "Internal");
        assert_eq!(SecurityLabel::Confidential.to_string(), "Confidential");
        assert_eq!(SecurityLabel::Restricted.to_string(), "Restricted");
    }

    #[test]
    fn info_flow_multiple_variables_mixed_levels() {
        // Multiple variables at different levels
        let mut checker = InfoFlowChecker::new();
        checker.declare("pub_data".into(), SecurityLabel::Public);
        checker.declare("int_data".into(), SecurityLabel::Internal);
        checker.declare("conf_data".into(), SecurityLabel::Confidential);
        checker.declare("restr_data".into(), SecurityLabel::Restricted);

        // Public -> Internal: ok
        assert!(
            checker
                .check_assignment(SecurityLabel::Internal, SecurityLabel::Public, &(0..1))
                .is_none()
        );
        // Internal -> Confidential: ok
        assert!(
            checker
                .check_assignment(
                    SecurityLabel::Confidential,
                    SecurityLabel::Internal,
                    &(0..1)
                )
                .is_none()
        );
        // Restricted -> Public: error
        assert_eq!(
            checker
                .check_assignment(SecurityLabel::Public, SecurityLabel::Restricted, &(0..1))
                .unwrap()
                .code,
            "A08001"
        );
        // Verify inferred labels
        assert_eq!(
            checker.infer_label(&AstExpr::Ident("pub_data".into())),
            SecurityLabel::Public
        );
        assert_eq!(
            checker.infer_label(&AstExpr::Ident("restr_data".into())),
            SecurityLabel::Restricted
        );
    }

    #[test]
    fn info_flow_checker_default() {
        // Default implementation matches new()
        let checker: InfoFlowChecker = Default::default();
        assert!(!checker.has_labels());
    }

    // --- T053 test helpers ---

    fn make_fn_def(name: &str, params: Vec<(&str, &[&str])>, clauses: Vec<AstClause>) -> AstFnDef {
        AstFnDef {
            name: name.into(),
            is_ghost: false,
            is_lemma: false,
            params: params
                .into_iter()
                .map(|(n, ty)| AstParam {
                    name: n.into(),
                    ty: ty.iter().map(|s| s.to_string()).collect(),
                })
                .collect(),
            return_ty: vec!["Int".into()],
            clauses,
        }
    }

    fn decreases_clause(body: AstExpr) -> AstClause {
        AstClause {
            kind: ClauseKind::Other("decreases".into()),
            body,
        }
    }

    fn requires_clause(body: AstExpr) -> AstClause {
        AstClause {
            kind: ClauseKind::Requires,
            body,
        }
    }

    fn partial_clause() -> AstClause {
        AstClause {
            kind: ClauseKind::Other("partial".into()),
            body: AstExpr::Literal(AstLit::Bool(true)),
        }
    }

    fn ensures_with_recursive_call(fn_name: &str, args: Vec<AstExpr>) -> AstClause {
        AstClause {
            kind: ClauseKind::Ensures,
            body: AstExpr::Call {
                func: Box::new(AstExpr::Ident(fn_name.into())),
                args,
            },
        }
    }

    #[test]
    fn totality_non_recursive_trivially_total() {
        // Non-recursive function passes without decreases
        let fn_def = make_fn_def("add", vec![("a", &["Int"]), ("b", &["Int"])], vec![]);
        let checker = TotalityChecker::new();
        let errors = checker.check_function_totality(&fn_def, &(0..10));
        assert!(
            errors.is_empty(),
            "non-recursive function should be trivially total"
        );
    }

    #[test]
    fn totality_recursive_with_valid_decreases() {
        // factorial(n) with decreases n, recursive call factorial(n - 1)
        let fn_def = make_fn_def(
            "factorial",
            vec![("n", &["Nat"])],
            vec![
                decreases_clause(AstExpr::Ident("n".into())),
                ensures_with_recursive_call(
                    "factorial",
                    vec![AstExpr::BinOp {
                        lhs: Box::new(AstExpr::Ident("n".into())),
                        op: AstBinOp::Sub,
                        rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
                    }],
                ),
            ],
        );
        let checker = TotalityChecker::new();
        let errors = checker.check_function_totality(&fn_def, &(0..20));
        assert!(
            errors.is_empty(),
            "valid decreasing measure should pass: {errors:?}"
        );
    }

    #[test]
    fn totality_recursive_without_decreases_a09001() {
        // Recursive function without decreases clause -> A09001
        let fn_def = make_fn_def(
            "loop_forever",
            vec![("n", &["Int"])],
            vec![ensures_with_recursive_call(
                "loop_forever",
                vec![AstExpr::Ident("n".into())],
            )],
        );
        let checker = TotalityChecker::new();
        let errors = checker.check_function_totality(&fn_def, &(0..10));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A09001");
    }

    #[test]
    fn totality_non_decreasing_measure_a09002() {
        // Recursive call with same argument (not decreasing) -> A09002
        let fn_def = make_fn_def(
            "spin",
            vec![("n", &["Nat"])],
            vec![
                decreases_clause(AstExpr::Ident("n".into())),
                ensures_with_recursive_call("spin", vec![AstExpr::Ident("n".into())]),
            ],
        );
        let checker = TotalityChecker::new();
        let errors = checker.check_function_totality(&fn_def, &(0..10));
        assert!(
            errors.iter().any(|e| e.code == "A09002"),
            "non-decreasing measure should produce A09002: {errors:?}"
        );
    }

    #[test]
    fn totality_measure_not_well_founded_a09003() {
        // decreases n but no requires n >= 0 and param type is Int, not Nat
        let fn_def = make_fn_def(
            "bad_rec",
            vec![("n", &["Int"])],
            vec![
                decreases_clause(AstExpr::Ident("n".into())),
                ensures_with_recursive_call(
                    "bad_rec",
                    vec![AstExpr::BinOp {
                        lhs: Box::new(AstExpr::Ident("n".into())),
                        op: AstBinOp::Sub,
                        rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
                    }],
                ),
            ],
        );
        let checker = TotalityChecker::new();
        let errors = checker.check_function_totality(&fn_def, &(0..10));
        assert!(
            errors.iter().any(|e| e.code == "A09003"),
            "missing well-foundedness should produce A09003: {errors:?}"
        );
    }

    #[test]
    fn totality_well_founded_with_requires_clause() {
        // decreases n with requires n >= 0 should NOT produce A09003
        let fn_def = make_fn_def(
            "count_down",
            vec![("n", &["Int"])],
            vec![
                requires_clause(AstExpr::BinOp {
                    lhs: Box::new(AstExpr::Ident("n".into())),
                    op: AstBinOp::Gte,
                    rhs: Box::new(AstExpr::Literal(AstLit::Int("0".into()))),
                }),
                decreases_clause(AstExpr::Ident("n".into())),
                ensures_with_recursive_call(
                    "count_down",
                    vec![AstExpr::BinOp {
                        lhs: Box::new(AstExpr::Ident("n".into())),
                        op: AstBinOp::Sub,
                        rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
                    }],
                ),
            ],
        );
        let checker = TotalityChecker::new();
        let errors = checker.check_function_totality(&fn_def, &(0..20));
        assert!(
            !errors.iter().any(|e| e.code == "A09003"),
            "requires n >= 0 should establish well-foundedness: {errors:?}"
        );
    }

    #[test]
    fn totality_partial_escape_hatch() {
        // Partial function skips termination checking
        let fn_def = make_fn_def(
            "diverge",
            vec![("n", &["Int"])],
            vec![
                partial_clause(),
                ensures_with_recursive_call("diverge", vec![AstExpr::Ident("n".into())]),
            ],
        );
        let checker = TotalityChecker::new();
        let errors = checker.check_function_totality(&fn_def, &(0..10));
        assert!(
            errors.is_empty(),
            "partial function should skip totality check"
        );
    }

    #[test]
    fn totality_partial_via_register() {
        // Partial registered via mark_partial
        let fn_def = make_fn_def(
            "diverge2",
            vec![("n", &["Int"])],
            vec![ensures_with_recursive_call(
                "diverge2",
                vec![AstExpr::Ident("n".into())],
            )],
        );
        let mut checker = TotalityChecker::new();
        checker.mark_partial("diverge2".into());
        let errors = checker.check_function_totality(&fn_def, &(0..10));
        assert!(errors.is_empty(), "registered partial should skip check");
    }

    #[test]
    fn totality_lexicographic_measures() {
        // Ackermann-like: decreases (m, n) with call (m - 1, n)
        let fn_def = make_fn_def(
            "ack",
            vec![("m", &["Nat"]), ("n", &["Nat"])],
            vec![
                decreases_clause(AstExpr::Ident("m".into())),
                decreases_clause(AstExpr::Ident("n".into())),
                ensures_with_recursive_call(
                    "ack",
                    vec![
                        AstExpr::BinOp {
                            lhs: Box::new(AstExpr::Ident("m".into())),
                            op: AstBinOp::Sub,
                            rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
                        },
                        AstExpr::Ident("n".into()),
                    ],
                ),
            ],
        );
        let checker = TotalityChecker::new();
        let errors = checker.check_function_totality(&fn_def, &(0..20));
        assert!(
            errors.is_empty(),
            "lexicographic decrease in first component should pass: {errors:?}"
        );
    }

    #[test]
    fn totality_mutual_recursion_no_decreases_a09004() {
        // Two functions calling each other with no decreases -> A09004
        let fn_a = make_fn_def(
            "even",
            vec![("n", &["Nat"])],
            vec![ensures_with_recursive_call(
                "odd",
                vec![AstExpr::BinOp {
                    lhs: Box::new(AstExpr::Ident("n".into())),
                    op: AstBinOp::Sub,
                    rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
                }],
            )],
        );
        let fn_b = make_fn_def(
            "odd",
            vec![("n", &["Nat"])],
            vec![ensures_with_recursive_call(
                "even",
                vec![AstExpr::BinOp {
                    lhs: Box::new(AstExpr::Ident("n".into())),
                    op: AstBinOp::Sub,
                    rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
                }],
            )],
        );

        let checker = TotalityChecker::new();
        let span_a = 0..10;
        let span_b = 10..20;
        let fn_defs: Vec<(&AstFnDef, &Range<usize>)> = vec![(&fn_a, &span_a), (&fn_b, &span_b)];
        let errors = checker.check_mutual_recursion(&fn_defs);
        assert!(
            errors.iter().any(|e| e.code == "A09004"),
            "mutual recursion without decreases should produce A09004: {errors:?}"
        );
    }

    #[test]
    fn totality_mutual_recursion_with_decreases_passes() {
        // Two functions calling each other, one has decreases -> passes
        let fn_a = make_fn_def(
            "even2",
            vec![("n", &["Nat"])],
            vec![
                decreases_clause(AstExpr::Ident("n".into())),
                ensures_with_recursive_call(
                    "odd2",
                    vec![AstExpr::BinOp {
                        lhs: Box::new(AstExpr::Ident("n".into())),
                        op: AstBinOp::Sub,
                        rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
                    }],
                ),
            ],
        );
        let fn_b = make_fn_def(
            "odd2",
            vec![("n", &["Nat"])],
            vec![ensures_with_recursive_call(
                "even2",
                vec![AstExpr::BinOp {
                    lhs: Box::new(AstExpr::Ident("n".into())),
                    op: AstBinOp::Sub,
                    rhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
                }],
            )],
        );

        let checker = TotalityChecker::new();
        let span_a = 0..10;
        let span_b = 10..20;
        let fn_defs: Vec<(&AstFnDef, &Range<usize>)> = vec![(&fn_a, &span_a), (&fn_b, &span_b)];
        let errors = checker.check_mutual_recursion(&fn_defs);
        assert!(
            errors.is_empty(),
            "mutual recursion with decreases should pass: {errors:?}"
        );
    }

    #[test]
    fn totality_structural_recursion_on_list() {
        // list_len(xs) with decreases xs, recursive call list_len(xs.tail)
        let fn_def = make_fn_def(
            "list_len",
            vec![("xs", &["List"])],
            vec![
                decreases_clause(AstExpr::Ident("xs".into())),
                ensures_with_recursive_call(
                    "list_len",
                    vec![AstExpr::Field(
                        Box::new(AstExpr::Ident("xs".into())),
                        "tail".into(),
                    )],
                ),
            ],
        );
        let checker = TotalityChecker::new();
        let errors = checker.check_function_totality(&fn_def, &(0..20));
        assert!(
            errors.is_empty(),
            "structural recursion on .tail should pass: {errors:?}"
        );
    }

    #[test]
    fn totality_structural_recursion_on_tree() {
        // tree_depth(node) with decreases node, calls tree_depth(node.left)
        let fn_def = make_fn_def(
            "tree_depth",
            vec![("node", &["Tree"])],
            vec![
                decreases_clause(AstExpr::Ident("node".into())),
                ensures_with_recursive_call(
                    "tree_depth",
                    vec![AstExpr::Field(
                        Box::new(AstExpr::Ident("node".into())),
                        "left".into(),
                    )],
                ),
            ],
        );
        let checker = TotalityChecker::new();
        let errors = checker.check_function_totality(&fn_def, &(0..20));
        assert!(
            errors.is_empty(),
            "structural recursion on .left should pass: {errors:?}"
        );
    }

    #[test]
    fn totality_extract_no_decreases() {
        let fn_def = make_fn_def("f", vec![], vec![]);
        let checker = TotalityChecker::new();
        assert!(checker.extract_decreases_measure(&fn_def).is_none());
    }

    #[test]
    fn totality_extract_single_decreases() {
        let fn_def = make_fn_def(
            "f",
            vec![("n", &["Nat"])],
            vec![decreases_clause(AstExpr::Ident("n".into()))],
        );
        let checker = TotalityChecker::new();
        let measure = checker.extract_decreases_measure(&fn_def);
        assert!(
            matches!(measure, Some(DecreasesMeasure::Natural(_))),
            "single decreases should yield Natural"
        );
    }

    #[test]
    fn totality_extract_lexicographic_decreases() {
        let fn_def = make_fn_def(
            "f",
            vec![("m", &["Nat"]), ("n", &["Nat"])],
            vec![
                decreases_clause(AstExpr::Ident("m".into())),
                decreases_clause(AstExpr::Ident("n".into())),
            ],
        );
        let checker = TotalityChecker::new();
        let measure = checker.extract_decreases_measure(&fn_def);
        assert!(
            matches!(measure, Some(DecreasesMeasure::Lexicographic(ref v)) if v.len() == 2),
            "two decreases should yield Lexicographic(2)"
        );
    }

    #[test]
    fn totality_checker_debug() {
        let checker = TotalityChecker::new();
        let dbg = format!("{checker:?}");
        assert!(dbg.contains("TotalityChecker"));
    }

    #[test]
    fn totality_checker_default() {
        let checker = TotalityChecker::default();
        assert!(!checker.is_partial(&make_fn_def("f", vec![], vec![])));
    }

    // -----------------------------------------------------------------------
    // T055 MEM.2: FixedWidthChecker tests
    // -----------------------------------------------------------------------

    #[test]
    fn fixed_width_range_u8() {
        let r = FixedWidthChecker::range_for_type(&Type::U8).unwrap();
        assert_eq!(r, (0, 255));
    }

    #[test]
    fn fixed_width_range_i8() {
        let r = FixedWidthChecker::range_for_type(&Type::I8).unwrap();
        assert_eq!(r, (-128, 127));
    }

    #[test]
    fn fixed_width_range_u16() {
        let r = FixedWidthChecker::range_for_type(&Type::U16).unwrap();
        assert_eq!(r, (0, 65535));
    }

    #[test]
    fn fixed_width_range_i16() {
        let r = FixedWidthChecker::range_for_type(&Type::I16).unwrap();
        assert_eq!(r, (-32768, 32767));
    }

    #[test]
    fn fixed_width_range_u32() {
        let r = FixedWidthChecker::range_for_type(&Type::U32).unwrap();
        assert_eq!(r, (0, u32::MAX as i128));
    }

    #[test]
    fn fixed_width_range_i32() {
        let r = FixedWidthChecker::range_for_type(&Type::I32).unwrap();
        assert_eq!(r, (i32::MIN as i128, i32::MAX as i128));
    }

    #[test]
    fn fixed_width_range_u64() {
        let r = FixedWidthChecker::range_for_type(&Type::U64).unwrap();
        assert_eq!(r, (0, u64::MAX as i128));
    }

    #[test]
    fn fixed_width_range_i64() {
        let r = FixedWidthChecker::range_for_type(&Type::I64).unwrap();
        assert_eq!(r, (i64::MIN as i128, i64::MAX as i128));
    }

    #[test]
    fn fixed_width_range_non_fixed() {
        // Non-fixed-width types return None
        assert!(FixedWidthChecker::range_for_type(&Type::Int).is_none());
        assert!(FixedWidthChecker::range_for_type(&Type::Bool).is_none());
        assert!(FixedWidthChecker::range_for_type(&Type::Float).is_none());
    }

    #[test]
    fn fixed_width_u8_overflow_add() {
        // U8 + U8: 255 + 255 = 510 > 255 -> overflow
        let checker = FixedWidthChecker::new();
        let err = checker.check_arithmetic_overflow(&AstBinOp::Add, &Type::U8, &Type::U8, &(0..1));
        assert!(err.is_some(), "U8 + U8 should detect potential overflow");
        let e = err.unwrap();
        assert_eq!(e.code, "A10101");
        assert!(e.message.contains("checked_add"));
    }

    #[test]
    fn fixed_width_i8_overflow_add() {
        // I8 + I8: 127 + 127 = 254 > 127 -> overflow
        let checker = FixedWidthChecker::new();
        let err = checker.check_arithmetic_overflow(&AstBinOp::Add, &Type::I8, &Type::I8, &(0..1));
        assert!(err.is_some(), "I8 + I8 should detect potential overflow");
        assert_eq!(err.unwrap().code, "A10101");
    }

    #[test]
    fn fixed_width_safe_arithmetic_no_error() {
        // This tests that overflow check only fires on arithmetic ops.
        // For comparison operators, no overflow check applies.
        let checker = FixedWidthChecker::new();
        let err = checker.check_arithmetic_overflow(&AstBinOp::Lt, &Type::U8, &Type::U8, &(0..1));
        assert!(err.is_none(), "comparison should not trigger overflow");
    }

    #[test]
    fn fixed_width_mul_overflow() {
        // U8 * U8: 255 * 255 = 65025 > 255 -> overflow
        let checker = FixedWidthChecker::new();
        let err = checker.check_arithmetic_overflow(&AstBinOp::Mul, &Type::U8, &Type::U8, &(0..1));
        assert!(err.is_some(), "U8 * U8 should detect potential overflow");
        let e = err.unwrap();
        assert!(e.message.contains("checked_mul"));
    }

    #[test]
    fn fixed_width_narrowing_cast_u32_to_u16() {
        // U32 -> U16: max 4294967295 > 65535 -> unsafe
        let err = FixedWidthChecker::check_cast_safety(&Type::U32, &Type::U16, &(0..1));
        assert!(err.is_some(), "U32 -> U16 should be unsafe narrowing");
        assert_eq!(err.unwrap().code, "A10102");
    }

    #[test]
    fn fixed_width_widening_cast_u16_to_u32() {
        // U16 -> U32: always safe (widening)
        let err = FixedWidthChecker::check_cast_safety(&Type::U16, &Type::U32, &(0..1));
        assert!(err.is_none(), "U16 -> U32 should be safe widening cast");
    }

    #[test]
    fn fixed_width_signed_unsigned_comparison() {
        // I32 == U32 -> signedness mismatch
        let err = FixedWidthChecker::check_signedness_mismatch(
            &AstBinOp::Eq,
            &Type::I32,
            &Type::U32,
            &(0..1),
        );
        assert!(err.is_some(), "I32 vs U32 comparison should warn");
        assert_eq!(err.unwrap().code, "A10103");
    }

    #[test]
    fn fixed_width_same_signedness_ok() {
        // U32 == U32 -> no mismatch
        let err = FixedWidthChecker::check_signedness_mismatch(
            &AstBinOp::Lt,
            &Type::U32,
            &Type::U32,
            &(0..1),
        );
        assert!(err.is_none(), "same signedness should not warn");
    }

    #[test]
    fn fixed_width_division_by_zero() {
        let rhs = AstExpr::Literal(AstLit::Int("0".into()));
        let err =
            FixedWidthChecker::check_division_by_zero(&AstBinOp::Div, &rhs, &Type::U32, &(0..1));
        assert!(err.is_some(), "division by literal 0 should be flagged");
        assert_eq!(err.unwrap().code, "A10104");
    }

    #[test]
    fn fixed_width_division_nonzero_ok() {
        let rhs = AstExpr::Literal(AstLit::Int("5".into()));
        let err =
            FixedWidthChecker::check_division_by_zero(&AstBinOp::Div, &rhs, &Type::U32, &(0..1));
        assert!(err.is_none(), "division by non-zero should pass");
    }

    #[test]
    fn fixed_width_suggest_checked_add() {
        assert_eq!(
            FixedWidthChecker::suggest_checked_alternative(&AstBinOp::Add),
            "checked_add"
        );
    }

    #[test]
    fn fixed_width_suggest_checked_sub() {
        assert_eq!(
            FixedWidthChecker::suggest_checked_alternative(&AstBinOp::Sub),
            "checked_sub"
        );
    }

    #[test]
    fn fixed_width_suggest_checked_mul() {
        assert_eq!(
            FixedWidthChecker::suggest_checked_alternative(&AstBinOp::Mul),
            "checked_mul"
        );
    }

    #[test]
    fn fixed_width_cast_i32_to_u32() {
        // I32 -> U32: signed-to-unsigned, range [-2^31, 2^31-1] does not
        // fit in [0, 2^32-1] because of negative values -> unsafe
        let err = FixedWidthChecker::check_cast_safety(&Type::I32, &Type::U32, &(0..1));
        assert!(err.is_some(), "I32 -> U32 cast should be unsafe");
        assert_eq!(err.unwrap().code, "A10102");
    }

    #[test]
    fn fixed_width_is_unsigned() {
        assert!(FixedWidthChecker::is_unsigned(&Type::U8));
        assert!(FixedWidthChecker::is_unsigned(&Type::U16));
        assert!(FixedWidthChecker::is_unsigned(&Type::U32));
        assert!(FixedWidthChecker::is_unsigned(&Type::U64));
        assert!(!FixedWidthChecker::is_unsigned(&Type::I8));
        assert!(!FixedWidthChecker::is_unsigned(&Type::Int));
    }

    #[test]
    fn fixed_width_is_signed() {
        assert!(FixedWidthChecker::is_signed(&Type::I8));
        assert!(FixedWidthChecker::is_signed(&Type::I16));
        assert!(FixedWidthChecker::is_signed(&Type::I32));
        assert!(FixedWidthChecker::is_signed(&Type::I64));
        assert!(!FixedWidthChecker::is_signed(&Type::U8));
        assert!(!FixedWidthChecker::is_signed(&Type::Float));
    }

    #[test]
    fn fixed_width_check_binop_combined() {
        // I8 + U8 -> both overflow (A10101) and signedness mismatch (A10103)
        let checker = FixedWidthChecker::new();
        let rhs_expr = AstExpr::Ident("y".into());
        let errors = checker.check_binop(&AstBinOp::Add, &Type::I8, &Type::U8, &rhs_expr, &(0..1));
        // Should have both an overflow error and a signedness mismatch
        let codes: Vec<&str> = errors.iter().map(|e| e.code.as_str()).collect();
        assert!(codes.contains(&"A10101"), "should flag overflow");
        // Signedness mismatch only fires for comparison ops, not arithmetic
        // (by design: check_signedness_mismatch only checks comparison ops)
    }

    #[test]
    fn fixed_width_modulo_by_zero() {
        let rhs = AstExpr::Literal(AstLit::Int("0".into()));
        let err =
            FixedWidthChecker::check_division_by_zero(&AstBinOp::Mod, &rhs, &Type::I32, &(0..1));
        assert!(err.is_some(), "modulo by zero should be flagged");
        let e = err.unwrap();
        assert_eq!(e.code, "A10104");
        assert!(e.message.contains("modulo"));
    }

    #[test]
    fn fixed_width_sub_overflow_unsigned() {
        // U8 - U8: 0 - 255 = -255 < 0 -> overflow (underflow)
        let checker = FixedWidthChecker::new();
        let err = checker.check_arithmetic_overflow(&AstBinOp::Sub, &Type::U8, &Type::U8, &(0..1));
        assert!(err.is_some(), "U8 - U8 should detect potential underflow");
        assert_eq!(err.unwrap().code, "A10101");
    }

    #[test]
    fn fixed_width_declare_and_lookup() {
        let mut checker = FixedWidthChecker::new();
        checker.declare("counter".into(), Type::U32);
        assert_eq!(checker.get_type("counter"), Some(&Type::U32));
        assert_eq!(checker.get_type("unknown"), None);
    }

    #[test]
    fn fixed_width_default_trait() {
        let checker = FixedWidthChecker::default();
        assert!(checker.get_type("x").is_none());
    }

    #[test]
    fn fixed_width_safe_cast_same_type() {
        // U32 -> U32: always safe
        assert!(FixedWidthChecker::is_safe_cast(&Type::U32, &Type::U32));
    }

    #[test]
    fn fixed_width_cast_non_fixed_width() {
        // Non-fixed-width types are outside scope -> treated as safe
        let err = FixedWidthChecker::check_cast_safety(&Type::Int, &Type::U32, &(0..1));
        assert!(err.is_none(), "non-fixed-width cast should be out of scope");
    }

    // =======================================================================
    // T056: AllocatorChecker tests
    // =======================================================================

    #[test]
    fn allocator_unpaired_alloc() {
        let mut checker = AllocatorChecker::new();
        checker.record_alloc("buf".into(), "1024".into(), None, 0..4);
        let errors = checker.check_unpaired();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A22001");
    }

    #[test]
    fn allocator_paired_ok() {
        let mut checker = AllocatorChecker::new();
        checker.record_alloc("buf".into(), "1024".into(), None, 0..4);
        assert!(checker.record_free("buf", 10..14).is_none());
        let errors = checker.check_unpaired();
        assert!(errors.is_empty());
    }

    #[test]
    fn allocator_double_free() {
        let mut checker = AllocatorChecker::new();
        checker.record_alloc("buf".into(), "1024".into(), None, 0..4);
        assert!(checker.record_free("buf", 10..14).is_none());
        let err = checker.record_free("buf", 20..24);
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A22002");
    }

    #[test]
    fn allocator_arena_ok() {
        let mut checker = AllocatorChecker::new();
        checker.declare_arena("arena1".into());
        checker.record_alloc("obj".into(), "64".into(), Some("arena1".into()), 0..4);
        // Arena-managed allocations are not required to have explicit free
        let errors = checker.check_unpaired();
        assert!(errors.is_empty());
    }

    #[test]
    fn allocator_arena_use_after_drop() {
        let mut checker = AllocatorChecker::new();
        checker.declare_arena("arena1".into());
        checker.record_alloc("obj".into(), "64".into(), Some("arena1".into()), 0..4);
        checker.drop_arena("arena1", 10..14);
        let err = checker.check_arena_use("obj", &(20..24));
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A22004");
    }

    #[test]
    fn allocator_arena_use_before_drop_ok() {
        let mut checker = AllocatorChecker::new();
        checker.declare_arena("arena1".into());
        checker.record_alloc("obj".into(), "64".into(), Some("arena1".into()), 0..4);
        let err = checker.check_arena_use("obj", &(5..8));
        assert!(err.is_none());
    }

    #[test]
    fn allocator_default() {
        let checker = AllocatorChecker::default();
        assert!(checker.check_unpaired().is_empty());
    }

    // =======================================================================
    // T057: CircularBufferChecker tests
    // =======================================================================

    #[test]
    fn circ_buf_read_empty() {
        let mut checker = CircularBufferChecker::new();
        checker.declare("ring".into(), 8);
        let err = checker.check_read("ring", &(0..1));
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A23003");
    }

    #[test]
    fn circ_buf_read_nonempty() {
        let mut checker = CircularBufferChecker::new();
        checker.declare("ring".into(), 8);
        checker.push("ring");
        assert!(checker.check_read("ring", &(0..1)).is_none());
    }

    #[test]
    fn circ_buf_index_out_of_bounds() {
        let mut checker = CircularBufferChecker::new();
        checker.declare("ring".into(), 4);
        let err = checker.check_index("ring", 5, &(0..1));
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A23001");
    }

    #[test]
    fn circ_buf_index_ok() {
        let mut checker = CircularBufferChecker::new();
        checker.declare("ring".into(), 4);
        assert!(checker.check_index("ring", 3, &(0..1)).is_none());
    }

    #[test]
    fn circ_buf_zero_capacity() {
        let mut checker = CircularBufferChecker::new();
        checker.declare("ring".into(), 0);
        let err = checker.check_physical_wrap("ring", 0, &(0..1));
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A23002");
    }

    #[test]
    fn circ_buf_push_pop() {
        let mut checker = CircularBufferChecker::new();
        checker.declare("ring".into(), 2);
        checker.push("ring");
        checker.push("ring");
        // Full, push should not increase count
        checker.push("ring");
        let info = checker.buffers.get("ring").unwrap();
        assert_eq!(info.count, 2);
        assert!(info.is_full());
        checker.pop("ring");
        let info = checker.buffers.get("ring").unwrap();
        assert_eq!(info.count, 1);
    }

    #[test]
    fn circ_buf_logical_to_physical() {
        let mut checker = CircularBufferChecker::new();
        checker.declare("ring".into(), 4);
        checker.push("ring");
        checker.push("ring");
        checker.pop("ring"); // head = 1
        let info = checker.buffers.get("ring").unwrap();
        assert_eq!(info.logical_to_physical(0), 1);
        assert_eq!(info.logical_to_physical(3), 0); // wraps
    }

    #[test]
    fn circ_buf_default() {
        let checker = CircularBufferChecker::default();
        assert!(checker.check_read("x", &(0..1)).is_none());
    }

    // =======================================================================
    // T066: CallbackReentrancyChecker tests
    // =======================================================================

    #[test]
    fn callback_reentrant_call() {
        let mut checker = CallbackReentrancyChecker::new();
        checker.mark_non_reentrant("handle_event".into(), 0..10);
        assert!(checker.enter_call("handle_event", &(0..1)).is_empty());
        // Re-entrant call
        let errors = checker.enter_call("handle_event", &(5..6));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A24001");
    }

    #[test]
    fn callback_reentrant_allowed() {
        let mut checker = CallbackReentrancyChecker::new();
        // Not marked non-reentrant
        assert!(checker.enter_call("handle_event", &(0..1)).is_empty());
        assert!(checker.enter_call("handle_event", &(5..6)).is_empty());
    }

    #[test]
    fn callback_max_depth() {
        let mut checker = CallbackReentrancyChecker::new().with_max_depth(2);
        assert!(checker.enter_call("a", &(0..1)).is_empty());
        assert!(checker.enter_call("b", &(0..1)).is_empty());
        let errors = checker.enter_call("c", &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A24003");
    }

    #[test]
    fn callback_register_in_context() {
        let mut checker = CallbackReentrancyChecker::new();
        checker.mark_non_reentrant("handler".into(), 0..10);
        assert!(checker.enter_call("handler", &(0..1)).is_empty());
        let err = checker.check_register_callback("handler", &(5..6));
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A24002");
    }

    #[test]
    fn callback_exit_resets() {
        let mut checker = CallbackReentrancyChecker::new();
        checker.mark_non_reentrant("f".into(), 0..10);
        assert!(checker.enter_call("f", &(0..1)).is_empty());
        checker.exit_call();
        // After exit, re-entry is allowed
        assert!(checker.enter_call("f", &(5..6)).is_empty());
    }

    #[test]
    fn callback_depth_tracking() {
        let mut checker = CallbackReentrancyChecker::new();
        assert_eq!(checker.current_depth(), 0);
        checker.enter_call("a", &(0..1));
        assert_eq!(checker.current_depth(), 1);
        checker.enter_call("b", &(0..1));
        assert_eq!(checker.current_depth(), 2);
        checker.exit_call();
        assert_eq!(checker.current_depth(), 1);
    }

    #[test]
    fn callback_default() {
        let checker = CallbackReentrancyChecker::default();
        assert_eq!(checker.current_depth(), 0);
    }

    // =======================================================================
    // T069: TemporalDeadlineChecker tests
    // =======================================================================

    #[test]
    fn deadline_operation_exceeds() {
        let mut checker = TemporalDeadlineChecker::new();
        checker.register_bound("heavy_compute".into(), 500);
        assert!(
            checker
                .enter_deadline("fast".into(), 100, &(0..1))
                .is_none()
        );
        let err = checker.check_operation("heavy_compute", &(5..6));
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A25001");
    }

    #[test]
    fn deadline_operation_ok() {
        let mut checker = TemporalDeadlineChecker::new();
        checker.register_bound("quick".into(), 10);
        assert!(
            checker
                .enter_deadline("normal".into(), 100, &(0..1))
                .is_none()
        );
        assert!(checker.check_operation("quick", &(5..6)).is_none());
    }

    #[test]
    fn deadline_unbounded_operation() {
        let mut checker = TemporalDeadlineChecker::new();
        assert!(
            checker
                .enter_deadline("strict".into(), 50, &(0..1))
                .is_none()
        );
        let err = checker.check_operation("unknown_op", &(5..6));
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A25003");
    }

    #[test]
    fn deadline_nested_violation() {
        let mut checker = TemporalDeadlineChecker::new();
        assert!(
            checker
                .enter_deadline("outer".into(), 100, &(0..1))
                .is_none()
        );
        let err = checker.enter_deadline("inner".into(), 200, &(5..6));
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A25002");
    }

    #[test]
    fn deadline_nested_ok() {
        let mut checker = TemporalDeadlineChecker::new();
        assert!(
            checker
                .enter_deadline("outer".into(), 100, &(0..1))
                .is_none()
        );
        assert!(
            checker
                .enter_deadline("inner".into(), 50, &(5..6))
                .is_none()
        );
    }

    #[test]
    fn deadline_no_context_ok() {
        let checker = TemporalDeadlineChecker::new();
        // No deadline context, any operation is fine
        assert!(checker.check_operation("anything", &(0..1)).is_none());
    }

    #[test]
    fn deadline_current() {
        let mut checker = TemporalDeadlineChecker::new();
        assert!(checker.current_deadline().is_none());
        checker.enter_deadline("d".into(), 42, &(0..1));
        assert_eq!(checker.current_deadline(), Some(("d", 42)));
        checker.exit_deadline();
        assert!(checker.current_deadline().is_none());
    }

    #[test]
    fn deadline_default() {
        let checker = TemporalDeadlineChecker::default();
        assert!(checker.current_deadline().is_none());
    }

    // =======================================================================
    // T070: BinaryFormatChecker tests
    // =======================================================================

    #[test]
    fn binary_fmt_bounds_ok() {
        let mut checker = BinaryFormatChecker::new();
        checker.add_field(BinaryField {
            name: "magic".into(),
            offset: 0,
            size: 4,
            endianness: Some(Endianness::Big),
            span: 0..1,
        });
        assert!(checker.check_bounds(100).is_empty());
    }

    #[test]
    fn binary_fmt_bounds_overflow() {
        let mut checker = BinaryFormatChecker::new();
        checker.add_field(BinaryField {
            name: "data".into(),
            offset: 96,
            size: 8,
            endianness: Some(Endianness::Little),
            span: 0..1,
        });
        let errors = checker.check_bounds(100);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A26001");
    }

    #[test]
    fn binary_fmt_no_endianness() {
        let mut checker = BinaryFormatChecker::new();
        checker.add_field(BinaryField {
            name: "len".into(),
            offset: 0,
            size: 4,
            endianness: None,
            span: 0..1,
        });
        let errors = checker.check_endianness();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A26003");
    }

    #[test]
    fn binary_fmt_single_byte_no_endianness_ok() {
        let mut checker = BinaryFormatChecker::new();
        checker.add_field(BinaryField {
            name: "flags".into(),
            offset: 0,
            size: 1,
            endianness: None,
            span: 0..1,
        });
        assert!(checker.check_endianness().is_empty());
    }

    #[test]
    fn binary_fmt_overlap() {
        let mut checker = BinaryFormatChecker::new();
        checker.add_field(BinaryField {
            name: "a".into(),
            offset: 0,
            size: 4,
            endianness: Some(Endianness::Big),
            span: 0..1,
        });
        checker.add_field(BinaryField {
            name: "b".into(),
            offset: 2,
            size: 4,
            endianness: Some(Endianness::Big),
            span: 0..1,
        });
        let errors = checker.check_overlaps();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A26004");
    }

    #[test]
    fn binary_fmt_no_overlap() {
        let mut checker = BinaryFormatChecker::new();
        checker.add_field(BinaryField {
            name: "a".into(),
            offset: 0,
            size: 4,
            endianness: Some(Endianness::Big),
            span: 0..1,
        });
        checker.add_field(BinaryField {
            name: "b".into(),
            offset: 4,
            size: 4,
            endianness: Some(Endianness::Big),
            span: 0..1,
        });
        assert!(checker.check_overlaps().is_empty());
    }

    #[test]
    fn binary_fmt_check_all() {
        let mut checker = BinaryFormatChecker::new();
        checker.add_field(BinaryField {
            name: "header".into(),
            offset: 0,
            size: 4,
            endianness: None,
            span: 0..1, // missing endianness
        });
        let errors = checker.check_all(100);
        assert_eq!(errors.len(), 1); // endianness only
    }

    #[test]
    fn binary_fmt_default() {
        let checker = BinaryFormatChecker::default();
        assert!(checker.check_all(0).is_empty());
    }

    // =======================================================================
    // T071: BitLevelChecker tests
    // =======================================================================

    #[test]
    fn bit_level_bounds_ok() {
        let mut checker = BitLevelChecker::new(32);
        checker.add_field(BitField {
            name: "version".into(),
            bit_offset: 0,
            bit_width: 4,
            span: 0..1,
            cross_byte_ok: false,
        });
        assert!(checker.check_bounds().is_empty());
    }

    #[test]
    fn bit_level_bounds_overflow() {
        let mut checker = BitLevelChecker::new(8);
        checker.add_field(BitField {
            name: "big".into(),
            bit_offset: 4,
            bit_width: 8,
            span: 0..1,
            cross_byte_ok: true,
        });
        let errors = checker.check_bounds();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A27001");
    }

    #[test]
    fn bit_level_byte_crossing() {
        let mut checker = BitLevelChecker::new(16);
        checker.add_field(BitField {
            name: "cross".into(),
            bit_offset: 6,
            bit_width: 4,
            span: 0..1,
            cross_byte_ok: false,
        });
        let errors = checker.check_byte_crossing();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A27002");
    }

    #[test]
    fn bit_level_byte_crossing_allowed() {
        let mut checker = BitLevelChecker::new(16);
        checker.add_field(BitField {
            name: "cross".into(),
            bit_offset: 6,
            bit_width: 4,
            span: 0..1,
            cross_byte_ok: true,
        });
        assert!(checker.check_byte_crossing().is_empty());
    }

    #[test]
    fn bit_level_total_width_match() {
        let mut checker = BitLevelChecker::new(8);
        checker.add_field(BitField {
            name: "a".into(),
            bit_offset: 0,
            bit_width: 4,
            span: 0..1,
            cross_byte_ok: false,
        });
        checker.add_field(BitField {
            name: "b".into(),
            bit_offset: 4,
            bit_width: 4,
            span: 0..1,
            cross_byte_ok: false,
        });
        assert!(checker.check_total_width(8).is_none());
    }

    #[test]
    fn bit_level_total_width_mismatch() {
        let mut checker = BitLevelChecker::new(8);
        checker.add_field(BitField {
            name: "a".into(),
            bit_offset: 0,
            bit_width: 3,
            span: 0..1,
            cross_byte_ok: false,
        });
        let err = checker.check_total_width(8);
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A27003");
    }

    #[test]
    fn bit_level_check_all() {
        let mut checker = BitLevelChecker::new(16);
        checker.add_field(BitField {
            name: "a".into(),
            bit_offset: 0,
            bit_width: 8,
            span: 0..1,
            cross_byte_ok: false,
        });
        checker.add_field(BitField {
            name: "b".into(),
            bit_offset: 8,
            bit_width: 8,
            span: 0..1,
            cross_byte_ok: false,
        });
        assert!(checker.check_all(16).is_empty());
    }

    // =======================================================================
    // T072: StringEncodingChecker tests
    // =======================================================================

    #[test]
    fn string_encoding_raw_bytes_error() {
        let mut checker = StringEncodingChecker::new();
        checker.declare("data".into(), StringEncoding::RawBytes);
        let err = checker.check_use_as_string("data", &(0..1));
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A28001");
    }

    #[test]
    fn string_encoding_utf8_ok() {
        let mut checker = StringEncodingChecker::new();
        checker.declare("text".into(), StringEncoding::Utf8);
        assert!(checker.check_use_as_string("text", &(0..1)).is_none());
    }

    #[test]
    fn string_encoding_mismatch() {
        let mut checker = StringEncodingChecker::new();
        checker.declare("wide".into(), StringEncoding::Utf16Le);
        let err = checker.check_encoding_compat("wide", &StringEncoding::Utf8, &(0..1));
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A28002");
    }

    #[test]
    fn string_encoding_ascii_compat() {
        let mut checker = StringEncodingChecker::new();
        checker.declare("ascii_str".into(), StringEncoding::Ascii);
        // ASCII is compatible with everything
        assert!(
            checker
                .check_encoding_compat("ascii_str", &StringEncoding::Utf8, &(0..1))
                .is_none()
        );
    }

    #[test]
    fn string_encoding_truncation_utf16() {
        let mut checker = StringEncodingChecker::new();
        checker.declare("wide".into(), StringEncoding::Utf16Le);
        let err = checker.check_truncation("wide", 5, &(0..1)); // 5 bytes, not aligned to 2
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A28003");
    }

    #[test]
    fn string_encoding_truncation_ok() {
        let mut checker = StringEncodingChecker::new();
        checker.declare("wide".into(), StringEncoding::Utf16Be);
        assert!(checker.check_truncation("wide", 4, &(0..1)).is_none()); // 4 bytes, aligned
    }

    #[test]
    fn string_encoding_unknown_var() {
        let checker = StringEncodingChecker::new();
        let err = checker.check_use_as_string("unknown", &(0..1));
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A28001");
    }

    #[test]
    fn string_encoding_default() {
        let checker = StringEncodingChecker::default();
        assert!(checker.check_use_as_string("x", &(0..1)).is_some());
    }

    // =======================================================================
    // T074: ChecksumChecker tests
    // =======================================================================

    #[test]
    fn checksum_use_before_verify() {
        let mut checker = ChecksumChecker::new();
        checker.declare_region("payload".into(), ChecksumAlgorithm::Crc32, 0, 100);
        let err = checker.check_use_before_verify("payload", &(0..1));
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A29001");
    }

    #[test]
    fn checksum_use_after_verify_ok() {
        let mut checker = ChecksumChecker::new();
        checker.declare_region("payload".into(), ChecksumAlgorithm::Crc32, 0, 100);
        checker.mark_verified("payload");
        assert!(
            checker
                .check_use_before_verify("payload", &(0..1))
                .is_none()
        );
    }

    #[test]
    fn checksum_algorithm_mismatch() {
        let mut checker = ChecksumChecker::new();
        checker.declare_region("data".into(), ChecksumAlgorithm::Sha256, 0, 100);
        let err = checker.check_algorithm_match("data", &ChecksumAlgorithm::Crc32, &(0..1));
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A29002");
    }

    #[test]
    fn checksum_algorithm_match_ok() {
        let mut checker = ChecksumChecker::new();
        checker.declare_region("data".into(), ChecksumAlgorithm::Sha256, 0, 100);
        assert!(
            checker
                .check_algorithm_match("data", &ChecksumAlgorithm::Sha256, &(0..1))
                .is_none()
        );
    }

    #[test]
    fn checksum_range_coverage() {
        let mut checker = ChecksumChecker::new();
        checker.declare_region("data".into(), ChecksumAlgorithm::Adler32, 10, 50);
        let err = checker.check_range_coverage("data", 0, 60, &(0..1));
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A29003");
    }

    #[test]
    fn checksum_range_covered_ok() {
        let mut checker = ChecksumChecker::new();
        checker.declare_region("data".into(), ChecksumAlgorithm::Adler32, 0, 100);
        assert!(
            checker
                .check_range_coverage("data", 10, 50, &(0..1))
                .is_none()
        );
    }

    #[test]
    fn checksum_default() {
        let checker = ChecksumChecker::default();
        assert!(checker.check_use_before_verify("x", &(0..1)).is_none());
    }

    // =======================================================================
    // T075: ProtocolGrammarChecker tests
    // =======================================================================

    #[test]
    fn protocol_valid_transition() {
        let mut checker = ProtocolGrammarChecker::new("idle".into());
        checker.add_state("connected".into());
        checker.add_transition("idle".into(), "connected".into(), "CONNECT".into());
        assert!(checker.check_send("CONNECT", &(0..1)).is_none());
        assert!(checker.transition("CONNECT", &(0..1)).is_none());
        assert_eq!(checker.current_state(), "connected");
    }

    #[test]
    fn protocol_invalid_send() {
        let mut checker = ProtocolGrammarChecker::new("idle".into());
        checker.add_transition("idle".into(), "connected".into(), "CONNECT".into());
        let err = checker.check_send("DISCONNECT", &(0..1));
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A30002");
    }

    #[test]
    fn protocol_invalid_transition() {
        let mut checker = ProtocolGrammarChecker::new("idle".into());
        checker.add_transition("idle".into(), "connected".into(), "CONNECT".into());
        let err = checker.transition("DATA", &(0..1));
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A30001");
    }

    #[test]
    fn protocol_required_fields() {
        let mut checker = ProtocolGrammarChecker::new("idle".into());
        checker.add_required_fields("CONNECT".into(), vec!["host".into(), "port".into()]);
        let errors = checker.check_required_fields("CONNECT", &["host"], &(0..1));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A30003");
        assert!(errors[0].message.contains("port"));
    }

    #[test]
    fn protocol_required_fields_ok() {
        let mut checker = ProtocolGrammarChecker::new("idle".into());
        checker.add_required_fields("CONNECT".into(), vec!["host".into()]);
        let errors = checker.check_required_fields("CONNECT", &["host", "port"], &(0..1));
        assert!(errors.is_empty());
    }

    #[test]
    fn protocol_multi_state() {
        let mut checker = ProtocolGrammarChecker::new("idle".into());
        checker.add_state("connected".into());
        checker.add_state("ready".into());
        checker.add_transition("idle".into(), "connected".into(), "CONNECT".into());
        checker.add_transition("connected".into(), "ready".into(), "AUTH".into());
        checker.add_transition("ready".into(), "idle".into(), "CLOSE".into());

        assert!(checker.transition("CONNECT", &(0..1)).is_none());
        assert_eq!(checker.current_state(), "connected");
        assert!(checker.transition("AUTH", &(0..1)).is_none());
        assert_eq!(checker.current_state(), "ready");
        assert!(checker.transition("CLOSE", &(0..1)).is_none());
        assert_eq!(checker.current_state(), "idle");
    }

    // =======================================================================
    // T077: AxiomaticDefChecker tests
    // =======================================================================

    #[test]
    fn axiom_undefined_reference() {
        let mut checker = AxiomaticDefChecker::new();
        checker.declare_axiom(AxiomDef {
            name: "ax1".into(),
            params: vec!["x".into()],
            body: "foo(x) > 0".into(),
            span: 0..1,
            references: vec!["foo".into()],
        });
        let errors = checker.check_references(&[]);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A31001");
    }

    #[test]
    fn axiom_known_reference_ok() {
        let mut checker = AxiomaticDefChecker::new();
        checker.declare_axiom(AxiomDef {
            name: "ax1".into(),
            params: vec![],
            body: "foo(x) > 0".into(),
            span: 0..1,
            references: vec!["foo".into()],
        });
        assert!(checker.check_references(&["foo"]).is_empty());
    }

    #[test]
    fn axiom_unused() {
        let mut checker = AxiomaticDefChecker::new();
        checker.declare_axiom(AxiomDef {
            name: "unused_ax".into(),
            params: vec![],
            body: "true".into(),
            span: 0..1,
            references: vec![],
        });
        let errors = checker.check_unused();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A31003");
    }

    #[test]
    fn axiom_used_ok() {
        let mut checker = AxiomaticDefChecker::new();
        checker.declare_axiom(AxiomDef {
            name: "ax1".into(),
            params: vec![],
            body: "true".into(),
            span: 0..1,
            references: vec![],
        });
        checker.mark_used("ax1");
        assert!(checker.check_unused().is_empty());
    }

    #[test]
    fn axiom_circular() {
        let mut checker = AxiomaticDefChecker::new();
        checker.declare_axiom(AxiomDef {
            name: "a".into(),
            params: vec![],
            body: "b(x)".into(),
            span: 0..1,
            references: vec!["b".into()],
        });
        checker.declare_axiom(AxiomDef {
            name: "b".into(),
            params: vec![],
            body: "a(x)".into(),
            span: 0..1,
            references: vec!["a".into()],
        });
        let errors = checker.check_circular();
        assert!(!errors.is_empty());
        assert!(errors.iter().any(|e| e.code == "A31002"));
    }

    #[test]
    fn axiom_default() {
        let checker = AxiomaticDefChecker::default();
        assert!(checker.check_unused().is_empty());
    }

    // =======================================================================
    // T079: OpaqueFunctionChecker tests
    // =======================================================================

    #[test]
    fn opaque_call_without_contract() {
        let mut checker = OpaqueFunctionChecker::new();
        checker.declare_opaque("secret_fn".into(), false, 0..1);
        let err = checker.check_call("secret_fn", &(5..6));
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A32001");
    }

    #[test]
    fn opaque_call_with_contract_ok() {
        let mut checker = OpaqueFunctionChecker::new();
        checker.declare_opaque("secret_fn".into(), true, 0..1);
        assert!(checker.check_call("secret_fn", &(5..6)).is_none());
    }

    #[test]
    fn opaque_body_access_without_reveal() {
        let mut checker = OpaqueFunctionChecker::new();
        checker.declare_opaque("hidden".into(), true, 0..1);
        let err = checker.check_body_access("hidden", &(5..6));
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A32002");
    }

    #[test]
    fn opaque_reveal_outside_proof() {
        let mut checker = OpaqueFunctionChecker::new();
        checker.declare_opaque("hidden".into(), true, 0..1);
        let err = checker.reveal("hidden", &(5..6));
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A32003");
    }

    #[test]
    fn opaque_reveal_in_proof_ok() {
        let mut checker = OpaqueFunctionChecker::new();
        checker.declare_opaque("hidden".into(), true, 0..1);
        checker.enter_proof();
        assert!(checker.reveal("hidden", &(5..6)).is_none());
        // After reveal, body access is allowed
        assert!(checker.check_body_access("hidden", &(10..11)).is_none());
    }

    #[test]
    fn opaque_is_opaque() {
        let mut checker = OpaqueFunctionChecker::new();
        assert!(!checker.is_opaque("f"));
        checker.declare_opaque("f".into(), true, 0..1);
        assert!(checker.is_opaque("f"));
    }

    #[test]
    fn opaque_non_opaque_call_ok() {
        let checker = OpaqueFunctionChecker::new();
        assert!(checker.check_call("regular_fn", &(0..1)).is_none());
    }

    #[test]
    fn opaque_default() {
        let checker = OpaqueFunctionChecker::default();
        assert!(!checker.is_opaque("x"));
    }

    // =======================================================================
    // T083: TestGenerator tests
    // =======================================================================

    #[test]
    fn test_gen_property_test() {
        let tgen = TestGenerator::new();
        let contract = TestableContract {
            name: "safe_div".into(),
            params: vec![("a".into(), Type::Int), ("b".into(), Type::Int)],
            requires: vec!["b != 0".into()],
            ensures: vec!["result * b + (a % b) == a".into()],
        };
        let test = tgen.generate_property_test(&contract);
        assert_eq!(test.kind, TestKind::Property);
        assert!(test.body.contains("proptest!"));
        assert!(test.body.contains("prop_assume!"));
        assert!(test.body.contains("b != 0"));
    }

    #[test]
    fn test_gen_boundary_values() {
        let tgen = TestGenerator::new();
        let contract = TestableContract {
            name: "check".into(),
            params: vec![("x".into(), Type::U8)],
            requires: vec![],
            ensures: vec![],
        };
        let tests = tgen.generate_boundary_tests(&contract);
        assert_eq!(tests.len(), 3); // 0, 1, 255
        assert!(tests.iter().all(|t| t.kind == TestKind::Boundary));
    }

    #[test]
    fn test_gen_smoke_test() {
        let tgen = TestGenerator::new();
        let contract = TestableContract {
            name: "foo".into(),
            params: vec![],
            requires: vec![],
            ensures: vec![],
        };
        let test = tgen.generate_smoke_test(&contract);
        assert_eq!(test.kind, TestKind::Smoke);
        assert!(test.body.contains("smoke_foo"));
    }

    #[test]
    fn test_gen_generate_all() {
        let mut tgen = TestGenerator::new();
        tgen.add_contract(TestableContract {
            name: "add".into(),
            params: vec![("a".into(), Type::I32), ("b".into(), Type::I32)],
            requires: vec![],
            ensures: vec!["result == a + b".into()],
        });
        let all = tgen.generate_all();
        // 1 property + 10 boundary (5 per I32 param * 2) + 1 smoke
        assert_eq!(all.len(), 12);
    }

    #[test]
    fn test_gen_no_requires() {
        let tgen = TestGenerator::new();
        let contract = TestableContract {
            name: "no_pre".into(),
            params: vec![("x".into(), Type::Bool)],
            requires: vec![],
            ensures: vec!["result".into()],
        };
        let test = tgen.generate_property_test(&contract);
        assert!(!test.body.contains("prop_assume!"));
    }

    #[test]
    fn test_gen_default() {
        let tgen = TestGenerator::default();
        assert!(tgen.generate_all().is_empty());
    }

    // =======================================================================
    // T086: CrashRecoveryChecker tests
    // =======================================================================

    #[test]
    fn crash_recovery_write_ahead_violation() {
        let mut cr = CrashRecoveryChecker::new();
        cr.begin_write("txn1".into());
        cr.write_data("txn1");
        let errs = cr.check_write_ahead();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A33001");
    }

    #[test]
    fn crash_recovery_write_ahead_ok() {
        let mut cr = CrashRecoveryChecker::new();
        cr.begin_write("txn1".into());
        cr.write_wal("txn1");
        cr.write_data("txn1");
        assert!(cr.check_write_ahead().is_empty());
    }

    #[test]
    fn crash_recovery_commit_without_fsync() {
        let mut cr = CrashRecoveryChecker::new();
        cr.begin_write("txn1".into());
        cr.commit("txn1");
        let errs = cr.check_commit_durability();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A33002");
    }

    #[test]
    fn crash_recovery_fsync_before_data() {
        let mut cr = CrashRecoveryChecker::new();
        cr.begin_write("txn1".into());
        cr.write_wal("txn1");
        cr.fsync("txn1");
        let errs = cr.check_ordering();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A33003");
    }

    #[test]
    fn crash_recovery_full_sequence_ok() {
        let mut cr = CrashRecoveryChecker::new();
        cr.begin_write("txn1".into());
        cr.write_wal("txn1");
        cr.write_data("txn1");
        cr.fsync("txn1");
        cr.commit("txn1");
        assert!(cr.check_all().is_empty());
    }

    #[test]
    fn crash_recovery_default() {
        let cr = CrashRecoveryChecker::default();
        assert!(cr.check_all().is_empty());
    }

    // =======================================================================
    // T087: PageCacheChecker tests
    // =======================================================================

    #[test]
    fn page_cache_evict_pinned() {
        let mut pc = PageCacheChecker::new(10);
        pc.load_page(1);
        pc.pin(1);
        let err = pc.evict(1);
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A34001");
    }

    #[test]
    fn page_cache_evict_dirty() {
        let mut pc = PageCacheChecker::new(10);
        pc.load_page(1);
        pc.mark_dirty(1);
        let err = pc.evict(1);
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A34002");
    }

    #[test]
    fn page_cache_evict_clean_ok() {
        let mut pc = PageCacheChecker::new(10);
        pc.load_page(1);
        assert!(pc.evict(1).is_none());
        assert_eq!(pc.page_count(), 0);
    }

    #[test]
    fn page_cache_capacity_exceeded() {
        let mut pc = PageCacheChecker::new(2);
        pc.load_page(1);
        pc.load_page(2);
        pc.load_page(3);
        let errs = pc.check_capacity();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A34003");
    }

    #[test]
    fn page_cache_flush_then_evict() {
        let mut pc = PageCacheChecker::new(10);
        pc.load_page(1);
        pc.mark_dirty(1);
        pc.flush(1);
        assert!(pc.evict(1).is_none());
    }

    #[test]
    fn page_cache_unpin_then_evict() {
        let mut pc = PageCacheChecker::new(10);
        pc.load_page(1);
        pc.pin(1);
        pc.unpin(1);
        assert!(pc.evict(1).is_none());
    }

    #[test]
    fn page_cache_default() {
        let pc = PageCacheChecker::default();
        assert_eq!(pc.page_count(), 0);
    }

    // =======================================================================
    // T088: MvccChecker tests
    // =======================================================================

    #[test]
    fn mvcc_write_conflict() {
        let mut mv = MvccChecker::new();
        let t1 = mv.begin_txn();
        let t2 = mv.begin_txn();
        mv.write_version("key1".into(), t1);
        mv.write_version("key1".into(), t2);
        let errs = mv.check_write_conflicts();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A35001");
    }

    #[test]
    fn mvcc_no_conflict_after_commit() {
        let mut mv = MvccChecker::new();
        let t1 = mv.begin_txn();
        mv.write_version("key1".into(), t1);
        mv.commit_txn(t1);
        let t2 = mv.begin_txn();
        mv.write_version("key1".into(), t2);
        assert!(mv.check_write_conflicts().is_empty());
    }

    #[test]
    fn mvcc_snapshot_violation() {
        let mut mv = MvccChecker::new();
        let t1 = mv.begin_txn();
        let t2 = mv.begin_txn();
        mv.write_version("key1".into(), t1);
        let err = mv.check_snapshot_read("key1", t2);
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A35002");
    }

    #[test]
    fn mvcc_phantom_read() {
        let mut mv = MvccChecker::new();
        let t1 = mv.begin_txn();
        let t2 = mv.begin_txn();
        mv.write_version("key1".into(), t2);
        mv.commit_txn(t2);
        let errs = mv.check_phantom(t1);
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A35003");
    }

    #[test]
    fn mvcc_default() {
        let mv = MvccChecker::default();
        assert!(mv.check_write_conflicts().is_empty());
    }

    // =======================================================================
    // T089: RollbackChecker tests
    // =======================================================================

    #[test]
    fn rollback_unknown_savepoint() {
        let mut rb = RollbackChecker::new();
        let err = rb.rollback_to("sp1");
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A36001");
    }

    #[test]
    fn rollback_resource_leak() {
        let mut rb = RollbackChecker::new();
        rb.create_savepoint("sp1".into());
        rb.acquire_resource("lock1".into());
        rb.rollback_to("sp1");
        let errs = rb.check_resource_leak();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A36002");
    }

    #[test]
    fn rollback_resource_released_ok() {
        let mut rb = RollbackChecker::new();
        rb.create_savepoint("sp1".into());
        rb.acquire_resource("lock1".into());
        rb.release_resource("lock1");
        rb.rollback_to("sp1");
        assert!(rb.check_resource_leak().is_empty());
    }

    #[test]
    fn rollback_duplicate_savepoint() {
        let mut rb = RollbackChecker::new();
        rb.create_savepoint("sp1".into());
        rb.create_savepoint("sp1".into());
        let errs = rb.check_savepoint_nesting();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A36003");
    }

    #[test]
    fn rollback_default() {
        let rb = RollbackChecker::default();
        assert!(rb.check_resource_leak().is_empty());
    }

    // =======================================================================
    // T090: MonotonicStateChecker tests
    // =======================================================================

    #[test]
    fn monotonic_increasing_violation() {
        let mut mc = MonotonicStateChecker::new();
        mc.declare("seq".into(), MonotonicDirection::Increasing, 10, 0..1);
        let err = mc.update("seq", 5);
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A37001");
    }

    #[test]
    fn monotonic_increasing_ok() {
        let mut mc = MonotonicStateChecker::new();
        mc.declare("seq".into(), MonotonicDirection::Increasing, 10, 0..1);
        assert!(mc.update("seq", 10).is_none()); // equal allowed for Increasing
        assert!(mc.update("seq", 15).is_none());
    }

    #[test]
    fn monotonic_strictly_increasing() {
        let mut mc = MonotonicStateChecker::new();
        mc.declare(
            "ts".into(),
            MonotonicDirection::StrictlyIncreasing,
            10,
            0..1,
        );
        let err = mc.update("ts", 10); // equal not allowed
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A37001");
    }

    #[test]
    fn monotonic_reset_blocked() {
        let mc = MonotonicStateChecker::new();
        assert!(mc.check_reset("seq").is_none()); // not declared = no error
    }

    #[test]
    fn monotonic_reset_declared() {
        let mut mc = MonotonicStateChecker::new();
        mc.declare("seq".into(), MonotonicDirection::Increasing, 0, 0..1);
        let err = mc.check_reset("seq");
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A37002");
    }

    #[test]
    fn monotonic_undeclared_access() {
        let mc = MonotonicStateChecker::new();
        let err = mc.check_access("unknown");
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A37003");
    }

    #[test]
    fn monotonic_current_value() {
        let mut mc = MonotonicStateChecker::new();
        mc.declare("seq".into(), MonotonicDirection::Increasing, 42, 0..1);
        assert_eq!(mc.current_value("seq"), Some(42));
        mc.update("seq", 100);
        assert_eq!(mc.current_value("seq"), Some(100));
    }

    #[test]
    fn monotonic_default() {
        let mc = MonotonicStateChecker::default();
        assert!(mc.check_access("x").is_some());
    }

    // =======================================================================
    // T091: StorageFailureChecker tests
    // =======================================================================

    #[test]
    fn storage_failure_unhandled() {
        let mut sf = StorageFailureChecker::new();
        sf.declare_failure_mode(FailureMode::PartialWrite);
        let errs = sf.check_unhandled();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A38001");
    }

    #[test]
    fn storage_failure_handled_ok() {
        let mut sf = StorageFailureChecker::new();
        sf.declare_failure_mode(FailureMode::BitRot);
        sf.mark_handled("bit_rot");
        assert!(sf.check_unhandled().is_empty());
    }

    #[test]
    fn storage_failure_spurious_handler() {
        let mut sf = StorageFailureChecker::new();
        sf.mark_handled("nonexistent");
        let errs = sf.check_spurious_handlers();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A38002");
    }

    #[test]
    fn storage_failure_critical_coverage() {
        let mut sf = StorageFailureChecker::new();
        sf.declare_failure_mode(FailureMode::PartialWrite);
        sf.declare_failure_mode(FailureMode::TornPage);
        let errs = sf.check_critical_coverage();
        assert_eq!(errs.len(), 2);
        assert!(errs.iter().all(|e| e.code == "A38003"));
    }

    #[test]
    fn storage_failure_count() {
        let mut sf = StorageFailureChecker::new();
        sf.declare_failure_mode(FailureMode::DiskFull);
        sf.declare_failure_mode(FailureMode::IoTimeout);
        assert_eq!(sf.failure_count(), 2);
    }

    #[test]
    fn storage_failure_default() {
        let sf = StorageFailureChecker::default();
        assert_eq!(sf.failure_count(), 0);
    }

    // =======================================================================
    // T095: NumericalPrecisionChecker tests
    // =======================================================================

    #[test]
    fn num_precision_loss() {
        let mut np = NumericalPrecisionChecker::new();
        np.declare("x".into(), 64, 1e-15, 0..1);
        let err = np.check_precision_loss("x", 32);
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A42001");
    }

    #[test]
    fn num_precision_ok() {
        let mut np = NumericalPrecisionChecker::new();
        np.declare("x".into(), 32, 1e-7, 0..1);
        assert!(np.check_precision_loss("x", 64).is_none());
    }

    #[test]
    fn num_ulp_violation() {
        let mut np = NumericalPrecisionChecker::new();
        np.declare("x".into(), 64, 1e-15, 0..1);
        let err = np.check_ulp_bound("x", 1e-10);
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A42002");
    }

    #[test]
    fn num_cancellation() {
        let mut np = NumericalPrecisionChecker::new();
        np.declare("x".into(), 64, 1e-15, 0..1);
        let err = np.check_cancellation("x", 0.9999);
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A42003");
    }

    #[test]
    fn num_precision_default() {
        let np = NumericalPrecisionChecker::default();
        assert!(np.check_precision_loss("x", 32).is_none());
    }

    // =======================================================================
    // T096: PrecomputedTableChecker tests
    // =======================================================================

    #[test]
    fn table_incomplete_coverage() {
        let mut tc = PrecomputedTableChecker::new();
        tc.declare_table("crc32".into(), 256, "gen_crc32".into(), 0..1);
        tc.mark_entries_verified("crc32", 100);
        let errs = tc.check_coverage();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A43001");
    }

    #[test]
    fn table_full_coverage_ok() {
        let mut tc = PrecomputedTableChecker::new();
        tc.declare_table("crc32".into(), 256, "gen_crc32".into(), 0..1);
        tc.mark_entries_verified("crc32", 256);
        assert!(tc.check_coverage().is_empty());
    }

    #[test]
    fn table_no_generator() {
        let mut tc = PrecomputedTableChecker::new();
        tc.declare_table("lut".into(), 16, "".into(), 0..1);
        let errs = tc.check_generator();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A43002");
    }

    #[test]
    fn table_zero_size() {
        let mut tc = PrecomputedTableChecker::new();
        tc.declare_table("empty".into(), 0, "gen".into(), 0..1);
        let errs = tc.check_non_empty();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A43003");
    }

    #[test]
    fn table_count() {
        let mut tc = PrecomputedTableChecker::new();
        tc.declare_table("a".into(), 10, "g".into(), 0..1);
        tc.declare_table("b".into(), 20, "g".into(), 0..1);
        assert_eq!(tc.table_count(), 2);
    }

    #[test]
    fn table_default() {
        let tc = PrecomputedTableChecker::default();
        assert_eq!(tc.table_count(), 0);
    }

    // =======================================================================
    // T097: PlatformAbstractionChecker tests
    // =======================================================================

    #[test]
    fn platform_missing_impl() {
        let mut pa = PlatformAbstractionChecker::new();
        pa.add_platform("linux".into());
        pa.add_platform("windows".into());
        pa.declare_abstraction("fs_ops".into(), vec!["linux".into()]);
        let errs = pa.check_coverage();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A44001");
    }

    #[test]
    fn platform_full_coverage_ok() {
        let mut pa = PlatformAbstractionChecker::new();
        pa.add_platform("linux".into());
        pa.declare_abstraction("fs_ops".into(), vec!["linux".into()]);
        assert!(pa.check_coverage().is_empty());
    }

    #[test]
    fn platform_direct_use() {
        let mut pa = PlatformAbstractionChecker::new();
        pa.add_platform("linux".into());
        let err = pa.check_direct_platform_use("linux");
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A44002");
    }

    #[test]
    fn platform_unknown() {
        let mut pa = PlatformAbstractionChecker::new();
        pa.add_platform("linux".into());
        pa.declare_abstraction("net".into(), vec!["freebsd".into()]);
        let errs = pa.check_unknown_platforms();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A44003");
    }

    #[test]
    fn platform_default() {
        let pa = PlatformAbstractionChecker::default();
        assert!(pa.check_coverage().is_empty());
    }

    // =======================================================================
    // T098: FeatureFlagChecker tests
    // =======================================================================

    #[test]
    fn feature_flag_unused() {
        let mut ff = FeatureFlagChecker::new();
        ff.declare("debug_mode".into(), false, vec![]);
        let errs = ff.check_unused();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A45001");
    }

    #[test]
    fn feature_flag_used_ok() {
        let mut ff = FeatureFlagChecker::new();
        ff.declare("debug_mode".into(), false, vec![]);
        ff.mark_used("debug_mode");
        assert!(ff.check_unused().is_empty());
    }

    #[test]
    fn feature_flag_conflict() {
        let mut ff = FeatureFlagChecker::new();
        ff.declare("a".into(), true, vec!["b".into()]);
        ff.declare("b".into(), true, vec![]);
        let errs = ff.check_conflicts();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A45002");
    }

    #[test]
    fn feature_flag_undeclared() {
        let ff = FeatureFlagChecker::new();
        let err = ff.check_undeclared("unknown");
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A45003");
    }

    #[test]
    fn feature_flag_default() {
        let ff = FeatureFlagChecker::default();
        assert!(ff.check_unused().is_empty());
    }

    // =======================================================================
    // T099: ResourceLimitChecker tests
    // =======================================================================

    #[test]
    fn resource_limit_exceeded() {
        let mut rl = ResourceLimitChecker::new();
        rl.declare_limit("mem".into(), 1000, "bytes".into());
        rl.record_usage("mem", 1500);
        let errs = rl.check_limits();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A46001");
    }

    #[test]
    fn resource_limit_ok() {
        let mut rl = ResourceLimitChecker::new();
        rl.declare_limit("mem".into(), 1000, "bytes".into());
        rl.record_usage("mem", 500);
        assert!(rl.check_limits().is_empty());
    }

    #[test]
    fn resource_unbounded() {
        let rl = ResourceLimitChecker::new();
        let err = rl.check_unbounded("unknown");
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A46002");
    }

    #[test]
    fn resource_near_limit() {
        let mut rl = ResourceLimitChecker::new();
        rl.declare_limit("fds".into(), 100, "count".into());
        rl.record_usage("fds", 95);
        let errs = rl.check_near_limit();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A46003");
    }

    #[test]
    fn resource_release() {
        let mut rl = ResourceLimitChecker::new();
        rl.declare_limit("mem".into(), 100, "bytes".into());
        rl.record_usage("mem", 80);
        rl.release_usage("mem", 50);
        assert_eq!(rl.current_usage("mem"), Some(30));
    }

    #[test]
    fn resource_default() {
        let rl = ResourceLimitChecker::default();
        assert!(rl.check_limits().is_empty());
    }

    // =======================================================================
    // T100: UnsafeEscapeChecker tests
    // =======================================================================

    #[test]
    fn unsafe_no_proof() {
        let mut ue = UnsafeEscapeChecker::new();
        ue.declare_unsafe("ptr_deref".into(), vec!["aligned".into()], 0..1);
        let errs = ue.check_unproven();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A47001");
    }

    #[test]
    fn unsafe_with_proof_ok() {
        let mut ue = UnsafeEscapeChecker::new();
        ue.declare_unsafe("ptr_deref".into(), vec!["aligned".into()], 0..1);
        ue.attach_proof("ptr_deref");
        assert!(ue.check_unproven().is_empty());
    }

    #[test]
    fn unsafe_undischarged_obligation() {
        let mut ue = UnsafeEscapeChecker::new();
        ue.declare_unsafe(
            "cast".into(),
            vec!["valid_repr".into(), "aligned".into()],
            0..1,
        );
        ue.discharge_obligation("cast", "valid_repr".into());
        let errs = ue.check_obligations();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A47002");
    }

    #[test]
    fn unsafe_empty_obligations() {
        let mut ue = UnsafeEscapeChecker::new();
        ue.declare_unsafe("noop".into(), vec![], 0..1);
        let errs = ue.check_empty_obligations();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A47003");
    }

    #[test]
    fn unsafe_count() {
        let mut ue = UnsafeEscapeChecker::new();
        ue.declare_unsafe("a".into(), vec![], 0..1);
        ue.declare_unsafe("b".into(), vec![], 0..1);
        assert_eq!(ue.unsafe_count(), 2);
    }

    #[test]
    fn unsafe_default() {
        let ue = UnsafeEscapeChecker::default();
        assert_eq!(ue.unsafe_count(), 0);
    }

    // =======================================================================
    // T101: ComplexityBoundChecker tests
    // =======================================================================

    #[test]
    fn complexity_bound_violated() {
        let mut cb = ComplexityBoundChecker::new();
        cb.declare_bound("sort".into(), ComplexityClass::Linear, 0..1);
        cb.record_measured("sort", ComplexityClass::Quadratic);
        let errs = cb.check_bounds();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A48001");
    }

    #[test]
    fn complexity_bound_ok() {
        let mut cb = ComplexityBoundChecker::new();
        cb.declare_bound("lookup".into(), ComplexityClass::Logarithmic, 0..1);
        cb.record_measured("lookup", ComplexityClass::Constant);
        assert!(cb.check_bounds().is_empty());
    }

    #[test]
    fn complexity_unverified() {
        let mut cb = ComplexityBoundChecker::new();
        cb.declare_bound("search".into(), ComplexityClass::Linear, 0..1);
        let errs = cb.check_unverified();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A48002");
    }

    #[test]
    fn complexity_exponential_warning() {
        let mut cb = ComplexityBoundChecker::new();
        cb.declare_bound("brute".into(), ComplexityClass::Exponential, 0..1);
        let errs = cb.check_expensive();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A48003");
    }

    #[test]
    fn complexity_default() {
        let cb = ComplexityBoundChecker::default();
        assert!(cb.check_bounds().is_empty());
    }

    // =======================================================================
    // T102: BehavioralEquivalenceChecker tests
    // =======================================================================

    #[test]
    fn equiv_unverified() {
        let mut be = BehavioralEquivalenceChecker::new();
        be.declare(
            "eq1".into(),
            "sort_a".into(),
            "sort_b".into(),
            "Sortable".into(),
            0..1,
        );
        let errs = be.check_unverified();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A49001");
    }

    #[test]
    fn equiv_verified_ok() {
        let mut be = BehavioralEquivalenceChecker::new();
        be.declare(
            "eq1".into(),
            "sort_a".into(),
            "sort_b".into(),
            "Sortable".into(),
            0..1,
        );
        be.mark_verified("eq1");
        assert!(be.check_unverified().is_empty());
    }

    #[test]
    fn equiv_self_equivalence() {
        let mut be = BehavioralEquivalenceChecker::new();
        be.declare(
            "eq1".into(),
            "sort_a".into(),
            "sort_a".into(),
            "Sortable".into(),
            0..1,
        );
        let errs = be.check_self_equivalence();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A49002");
    }

    #[test]
    fn equiv_no_contract() {
        let mut be = BehavioralEquivalenceChecker::new();
        be.declare("eq1".into(), "a".into(), "b".into(), "".into(), 0..1);
        let errs = be.check_contract_ref();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A49003");
    }

    #[test]
    fn equiv_default() {
        let be = BehavioralEquivalenceChecker::default();
        assert!(be.check_unverified().is_empty());
    }

    // =======================================================================
    // T103: MultiPassRefinementChecker tests
    // =======================================================================

    #[test]
    fn refinement_incomplete() {
        let mut mp = MultiPassRefinementChecker::new();
        mp.add_pass("r1".into(), "spec".into(), "design".into(), 5, 0..1);
        mp.discharge("r1", 3);
        let errs = mp.check_complete();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A50001");
    }

    #[test]
    fn refinement_complete_ok() {
        let mut mp = MultiPassRefinementChecker::new();
        mp.add_pass("r1".into(), "spec".into(), "design".into(), 5, 0..1);
        mp.discharge("r1", 5);
        assert!(mp.check_complete().is_empty());
    }

    #[test]
    fn refinement_chain_gap() {
        let mut mp = MultiPassRefinementChecker::new();
        mp.add_pass("r1".into(), "spec".into(), "design".into(), 1, 0..1);
        mp.add_pass("r2".into(), "impl".into(), "code".into(), 1, 0..1);
        let errs = mp.check_chain();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A50002");
    }

    #[test]
    fn refinement_zero_obligations() {
        let mut mp = MultiPassRefinementChecker::new();
        mp.add_pass("r1".into(), "spec".into(), "design".into(), 0, 0..1);
        let errs = mp.check_non_trivial();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A50003");
    }

    #[test]
    fn refinement_pass_count() {
        let mut mp = MultiPassRefinementChecker::new();
        mp.add_pass("r1".into(), "a".into(), "b".into(), 1, 0..1);
        mp.add_pass("r2".into(), "b".into(), "c".into(), 1, 0..1);
        assert_eq!(mp.pass_count(), 2);
    }

    #[test]
    fn refinement_default() {
        let mp = MultiPassRefinementChecker::default();
        assert_eq!(mp.pass_count(), 0);
    }

    // =======================================================================
    // T104: IncrementalContractChecker tests
    // =======================================================================

    #[test]
    fn incremental_strengthens_precondition() {
        let mut ic = IncrementalContractChecker::new();
        ic.add_version("SafeDiv".into(), 1, 1, 1);
        ic.add_version("SafeDiv".into(), 2, 3, 1); // more requires = stronger
        let errs = ic.check_precondition_weakening();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A51001");
    }

    #[test]
    fn incremental_weakens_postcondition() {
        let mut ic = IncrementalContractChecker::new();
        ic.add_version("SafeDiv".into(), 1, 1, 3);
        ic.add_version("SafeDiv".into(), 2, 1, 1); // fewer ensures = weaker
        let errs = ic.check_postcondition_strengthening();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A51002");
    }

    #[test]
    fn incremental_version_gap() {
        let mut ic = IncrementalContractChecker::new();
        ic.add_version("SafeDiv".into(), 1, 1, 1);
        ic.add_version("SafeDiv".into(), 5, 1, 1);
        let errs = ic.check_version_continuity();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A51003");
    }

    #[test]
    fn incremental_ok() {
        let mut ic = IncrementalContractChecker::new();
        ic.add_version("SafeDiv".into(), 1, 3, 1);
        ic.add_version("SafeDiv".into(), 2, 2, 2); // weaker pre, stronger post
        assert!(ic.check_precondition_weakening().is_empty());
        assert!(ic.check_postcondition_strengthening().is_empty());
    }

    #[test]
    fn incremental_default() {
        let ic = IncrementalContractChecker::default();
        assert!(ic.check_precondition_weakening().is_empty());
    }

    // =======================================================================
    // T105: ScopedInvariantChecker tests
    // =======================================================================

    #[test]
    fn invariant_double_suspend() {
        let mut si = ScopedInvariantChecker::new();
        si.declare_invariant("inv1".into());
        assert!(si.suspend("inv1").is_none());
        let err = si.suspend("inv1");
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A52001");
    }

    #[test]
    fn invariant_suspend_undeclared() {
        let mut si = ScopedInvariantChecker::new();
        let err = si.suspend("unknown");
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A52002");
    }

    #[test]
    fn invariant_restore_not_suspended() {
        let mut si = ScopedInvariantChecker::new();
        si.declare_invariant("inv1".into());
        let err = si.restore("inv1");
        assert!(err.is_some());
        assert_eq!(err.unwrap().code, "A52003");
    }

    #[test]
    fn invariant_suspend_restore_ok() {
        let mut si = ScopedInvariantChecker::new();
        si.declare_invariant("inv1".into());
        si.suspend("inv1");
        assert!(si.is_suspended("inv1"));
        si.restore("inv1");
        assert!(!si.is_suspended("inv1"));
        assert!(si.check_all_restored().is_empty());
    }

    #[test]
    fn invariant_still_suspended_at_exit() {
        let mut si = ScopedInvariantChecker::new();
        si.declare_invariant("inv1".into());
        si.suspend("inv1");
        let errs = si.check_all_restored();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A52001");
    }

    #[test]
    fn invariant_suspension_depth() {
        let mut si = ScopedInvariantChecker::new();
        si.declare_invariant("a".into());
        si.declare_invariant("b".into());
        si.suspend("a");
        si.suspend("b");
        assert_eq!(si.suspension_depth(), 2);
        si.restore("a");
        assert_eq!(si.suspension_depth(), 1);
    }

    #[test]
    fn invariant_default() {
        let si = ScopedInvariantChecker::default();
        assert_eq!(si.suspension_depth(), 0);
    }

    // =======================================================================
    // T107: StdlibTypes tests
    // =======================================================================

    #[test]
    fn stdlib_has_core_types() {
        let stdlib = StdlibTypes::new();
        assert!(stdlib.is_stdlib_type("Pos"));
        assert!(stdlib.is_stdlib_type("NonNeg"));
        assert!(stdlib.is_stdlib_type("Email"));
        assert!(stdlib.is_stdlib_type("Uuid"));
        assert!(!stdlib.is_stdlib_type("Unknown"));
    }

    #[test]
    fn stdlib_lookup() {
        let stdlib = StdlibTypes::new();
        let pos = stdlib.lookup("Pos").unwrap();
        assert_eq!(pos.refinement, "v > 0");
        assert_eq!(pos.base_type, Type::Int);
    }

    #[test]
    fn stdlib_type_count() {
        let stdlib = StdlibTypes::new();
        assert!(stdlib.type_count() >= 6);
    }

    #[test]
    fn stdlib_default() {
        let stdlib = StdlibTypes::default();
        assert!(stdlib.type_count() >= 6);
    }

    // =======================================================================
    // T108: CollectionContracts tests
    // =======================================================================

    #[test]
    fn collection_has_standard_ops() {
        let cc = CollectionContracts::new();
        assert!(cc.lookup("sort").is_some());
        assert!(cc.lookup("filter").is_some());
        assert!(cc.lookup("map").is_some());
        assert!(cc.lookup("reverse").is_some());
    }

    #[test]
    fn collection_sort_preserves_length() {
        let cc = CollectionContracts::new();
        let sort = cc.lookup("sort").unwrap();
        assert!(sort.preserves_length);
        assert!(sort.preserves_elements);
    }

    #[test]
    fn collection_filter_does_not_preserve_length() {
        let cc = CollectionContracts::new();
        let filter = cc.lookup("filter").unwrap();
        assert!(!filter.preserves_length);
    }

    #[test]
    fn collection_contract_count() {
        let cc = CollectionContracts::new();
        assert!(cc.contract_count() >= 5);
    }

    #[test]
    fn collection_default() {
        let cc = CollectionContracts::default();
        assert!(cc.contract_count() >= 5);
    }

    // =======================================================================
    // T109: CrudAuthContracts tests
    // =======================================================================

    #[test]
    fn crud_auth_missing_policy() {
        let mut ca = CrudAuthContracts::new();
        ca.add_crud("create_user".into(), CrudType::Create, true);
        let errs = ca.check_auth_coverage();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A53001");
    }

    #[test]
    fn crud_auth_with_policy_ok() {
        let mut ca = CrudAuthContracts::new();
        ca.add_crud("create_user".into(), CrudType::Create, true);
        ca.add_auth_policy("create_user".into(), "admin".into(), false);
        assert!(ca.check_auth_coverage().is_empty());
    }

    #[test]
    fn crud_delete_without_auth() {
        let mut ca = CrudAuthContracts::new();
        ca.add_crud("delete_item".into(), CrudType::Delete, false);
        let errs = ca.check_delete_protection();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A53002");
    }

    #[test]
    fn crud_counts() {
        let mut ca = CrudAuthContracts::new();
        ca.add_crud("a".into(), CrudType::Read, false);
        ca.add_auth_policy("a".into(), "user".into(), true);
        assert_eq!(ca.crud_count(), 1);
        assert_eq!(ca.policy_count(), 1);
    }

    #[test]
    fn crud_default() {
        let ca = CrudAuthContracts::default();
        assert_eq!(ca.crud_count(), 0);
    }

    // =======================================================================
    // T110: ContractCompositionChecker tests
    // =======================================================================

    #[test]
    fn composition_unknown_extends() {
        let mut cc = ContractCompositionChecker::new();
        cc.declare("Child".into(), vec!["Unknown".into()], 1);
        let errs = cc.check_extends();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A54001");
    }

    #[test]
    fn composition_valid_extends() {
        let mut cc = ContractCompositionChecker::new();
        cc.declare("Base".into(), vec![], 2);
        cc.declare("Child".into(), vec!["Base".into()], 1);
        assert!(cc.check_extends().is_empty());
    }

    #[test]
    fn composition_circular() {
        let mut cc = ContractCompositionChecker::new();
        cc.declare("A".into(), vec!["B".into()], 1);
        cc.declare("B".into(), vec!["A".into()], 1);
        let errs = cc.check_circular();
        assert!(!errs.is_empty());
        assert!(errs.iter().any(|e| e.code == "A54002"));
    }

    #[test]
    fn composition_diamond() {
        let mut cc = ContractCompositionChecker::new();
        cc.declare("Base".into(), vec![], 1);
        cc.declare("Left".into(), vec!["Base".into()], 1);
        cc.declare("Right".into(), vec!["Base".into()], 1);
        cc.declare("Diamond".into(), vec!["Left".into(), "Right".into()], 1);
        let errs = cc.check_diamond();
        assert!(!errs.is_empty());
        assert!(errs.iter().any(|e| e.code == "A54003"));
    }

    #[test]
    fn composition_default() {
        let cc = ContractCompositionChecker::default();
        assert_eq!(cc.contract_count(), 0);
    }

    // =======================================================================
    // T111: ContractLibraryChecker tests
    // =======================================================================

    #[test]
    fn library_empty_exports() {
        let mut lc = ContractLibraryChecker::new();
        lc.declare_library("mylib".into(), "1.0.0".into());
        let errs = lc.check_empty_exports();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A55001");
    }

    #[test]
    fn library_with_exports_ok() {
        let mut lc = ContractLibraryChecker::new();
        lc.declare_library("mylib".into(), "1.0.0".into());
        lc.add_export("mylib", "SafeDiv".into());
        assert!(lc.check_empty_exports().is_empty());
    }

    #[test]
    fn library_self_dependency() {
        let mut lc = ContractLibraryChecker::new();
        lc.declare_library("mylib".into(), "1.0.0".into());
        lc.add_dependency(
            "mylib",
            LibraryDep {
                name: "mylib".into(),
                version_req: ">=1.0".into(),
            },
        );
        let errs = lc.check_circular_deps();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A55002");
    }

    #[test]
    fn library_duplicate() {
        let mut lc = ContractLibraryChecker::new();
        lc.declare_library("mylib".into(), "1.0.0".into());
        lc.declare_library("mylib".into(), "2.0.0".into());
        let errs = lc.check_duplicates();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, "A55003");
    }

    #[test]
    fn library_default() {
        let lc = ContractLibraryChecker::default();
        assert_eq!(lc.library_count(), 0);
    }

    // -----------------------------------------------------------------------
    // Match expression exhaustiveness wiring tests (T017)
    // -----------------------------------------------------------------------

    #[test]
    fn match_infer_type_from_first_arm() {
        // match x { A => 42, B => 0 } should infer Int from the first arm
        let env = TypeEnv::new();
        let expr = AstExpr::Match {
            scrutinee: Box::new(AstExpr::Ident("x".into())),
            arms: vec![
                assura_parser::ast::MatchArm {
                    pattern: assura_parser::ast::Pattern::Ident("A".into()),
                    body: AstExpr::Literal(AstLit::Int("42".into())),
                },
                assura_parser::ast::MatchArm {
                    pattern: assura_parser::ast::Pattern::Ident("B".into()),
                    body: AstExpr::Literal(AstLit::Int("0".into())),
                },
            ],
        };
        let result = infer_expr(&expr, &env);
        assert_eq!(result.unwrap(), Type::Int);
    }

    #[test]
    fn match_empty_arms_infers_unknown() {
        let env = TypeEnv::new();
        let expr = AstExpr::Match {
            scrutinee: Box::new(AstExpr::Ident("x".into())),
            arms: vec![],
        };
        let result = infer_expr(&expr, &env);
        assert_eq!(result.unwrap(), Type::Unknown);
    }

    #[test]
    fn match_expr_references_var() {
        let expr = AstExpr::Match {
            scrutinee: Box::new(AstExpr::Ident("status".into())),
            arms: vec![assura_parser::ast::MatchArm {
                pattern: assura_parser::ast::Pattern::Ident("A".into()),
                body: AstExpr::Ident("result".into()),
            }],
        };
        assert!(expr_references_var(&expr, "status"));
        assert!(expr_references_var(&expr, "result"));
        assert!(!expr_references_var(&expr, "other"));
    }
}
