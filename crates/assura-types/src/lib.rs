//! Type checking for the Assura contract language.
//!
//! Builds a `TypeEnv` (type environment) from a `ResolvedFile` by mapping
//! each symbol in the symbol table to its `Type`. For T013 this creates the
//! scaffolding: type environment construction and the `type_check` entry
//! point. Actual expression-level type checking (T014-T018) builds on this.

use std::collections::HashMap;
use std::ops::Range;

use assura_parser::ast::{BinOp, Decl, Expr, Literal, ServiceItem, UnaryOp};
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
}

impl TypeEnv {
    /// Create an empty type environment.
    pub fn new() -> Self {
        Self {
            bindings: HashMap::new(),
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
// Type environment construction
// ---------------------------------------------------------------------------

/// Build a `TypeEnv` from a resolved symbol table.
///
/// Walks every symbol and assigns it a `Type` based on its kind:
/// - `BuiltinType`: mapped via `builtin_type()`
/// - `TypeDef`, `ContractDef`, `ServiceDef`, `EnumDef`: `Type::Named(name)`
/// - `FnDef`, `ExternFn`: `Type::Fn { params: [], ret: Unknown }`
///   (parameter types are not yet resolved from raw token sequences)
/// - `TypeParam`: `Type::TypeParam(name)`
/// - `Parameter`, `Field`: `Type::Unknown` (refined in T014+)
/// - `Operation`, `Query`: `Type::Fn { params: [], ret: Unknown }`
/// - `EnumVariant`: `Type::Named(name)` (constructor)
fn build_type_env(symbols: &SymbolTable) -> TypeEnv {
    let mut env = TypeEnv::new();

    for sym in &symbols.symbols {
        let ty = match sym.kind {
            SymbolKind::BuiltinType => builtin_type(&sym.name).unwrap_or(Type::Unknown),
            SymbolKind::TypeDef
            | SymbolKind::ContractDef
            | SymbolKind::ServiceDef
            | SymbolKind::EnumDef => Type::Named(sym.name.clone()),

            SymbolKind::FnDef | SymbolKind::ExternFn => Type::Fn {
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
        };

        env.insert(sym.name.clone(), ty);
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
// Expression type inference
// ---------------------------------------------------------------------------

/// Returns `true` if `ty` is a numeric type.
fn is_numeric(ty: &Type) -> bool {
    matches!(
        ty,
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
            | Type::F64
    )
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
        Expr::Field(receiver, _field) => {
            // Infer the receiver type. If it is Unknown or a Named type
            // (user-defined struct) we cannot resolve fields without a
            // richer TypeEnv that stores struct field info, so return
            // Unknown. We never emit A03004 here because we cannot
            // confirm whether the field exists or not yet.
            let _recv_ty = infer_expr(receiver, env)?;
            Ok(Type::Unknown)
        }

        // --- Method call ---
        Expr::MethodCall {
            receiver,
            method: _,
            args,
        } => {
            // Infer receiver and argument types (to surface errors
            // inside them) but return Unknown since full method
            // resolution requires struct/service context.
            let _recv_ty = infer_expr(receiver, env)?;
            for arg in args {
                let _ = infer_expr(arg, env)?;
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

        // --- Block / Raw: cannot infer ---
        Expr::Block(_) | Expr::Raw(_) => Ok(Type::Unknown),
    }
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
            if lhs_ty != rhs_ty {
                return Err(TypeError {
                    code: "A03001".into(),
                    message: format!("type mismatch in arithmetic: `{lhs_ty}` vs `{rhs_ty}`"),
                    span: 0..0,
                    secondary: None,
                });
            }
            Ok(lhs_ty)
        }

        // Comparison: operands same type, result Bool
        BinOp::Eq | BinOp::Neq | BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte => {
            if lhs_ty != rhs_ty {
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
// Clause body type checking
// ---------------------------------------------------------------------------

/// Walk all clause bodies in a source file, infer expression types, and
/// collect type errors. Lenient: errors involving `Unknown` are suppressed.
fn check_clause_bodies(source: &assura_parser::ast::SourceFile, env: &TypeEnv) -> Vec<TypeError> {
    let mut errors = Vec::new();

    for decl in &source.decls {
        match &decl.node {
            Decl::Contract(c) => {
                for clause in &c.clauses {
                    collect_expr_errors(&clause.body, env, &mut errors);
                }
            }
            Decl::FnDef(f) => {
                for clause in &f.clauses {
                    collect_expr_errors(&clause.body, env, &mut errors);
                }
            }
            Decl::Extern(ex) => {
                for clause in &ex.clauses {
                    collect_expr_errors(&clause.body, env, &mut errors);
                }
            }
            Decl::Service(s) => {
                for item in &s.items {
                    let clauses = match item {
                        ServiceItem::Operation { clauses, .. }
                        | ServiceItem::Query { clauses, .. } => clauses.as_slice(),
                        ServiceItem::Invariant(expr) => {
                            collect_expr_errors(expr, env, &mut errors);
                            continue;
                        }
                        ServiceItem::Other { body, .. } => {
                            collect_expr_errors(body, env, &mut errors);
                            continue;
                        }
                        _ => continue,
                    };
                    for clause in clauses {
                        collect_expr_errors(&clause.body, env, &mut errors);
                    }
                }
            }
            Decl::Block { body, .. } => {
                for clause in body {
                    collect_expr_errors(&clause.body, env, &mut errors);
                }
            }
            // TypeDef and EnumDef don't have expression bodies
            Decl::TypeDef(_) | Decl::EnumDef(_) => {}
        }
    }

    errors
}

/// Try to infer the type of an expression; if a type error occurs, push
/// it into the collector.
fn collect_expr_errors(expr: &Expr, env: &TypeEnv, errors: &mut Vec<TypeError>) {
    match infer_expr(expr, env) {
        Ok(_) => {}
        Err(e) => errors.push(e),
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
    let type_env = build_type_env(&resolved.symbols);

    // T014: walk clause bodies and infer expression types. Collect any
    // concrete type-mismatch errors (A03001). Unknown types from unresolved
    // identifiers are silently propagated (no false positives).
    let errors = check_clause_bodies(&resolved.source, &type_env);
    if !errors.is_empty() {
        return Err(errors);
    }

    Ok(TypedFile {
        resolved: resolved.clone(),
        type_env,
    })
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
// Tests
// ---------------------------------------------------------------------------

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
                params: Vec::new(),
                ret: Box::new(Type::Unknown),
            })
        );
        // Parameter gets Unknown for now
        assert_eq!(typed.type_env.lookup("n"), Some(&Type::Unknown));
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
    // T014: Expression type inference tests
    // -----------------------------------------------------------------------

    use assura_parser::ast::{
        BinOp as AstBinOp, Expr as AstExpr, Literal as AstLit, UnaryOp as AstUnOp,
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
    fn infer_arithmetic_mismatch() {
        let env = TypeEnv::new();
        let expr = AstExpr::BinOp {
            lhs: Box::new(AstExpr::Literal(AstLit::Int("1".into()))),
            op: AstBinOp::Add,
            rhs: Box::new(AstExpr::Literal(AstLit::Float("2.0".into()))),
        };
        let err = infer_expr(&expr, &env).unwrap_err();
        assert_eq!(err.code, "A03001");
        assert!(err.message.contains("Int"));
        assert!(err.message.contains("Float"));
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
        // Named type field access returns Unknown (no struct field info yet)
        assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unknown);
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
}
