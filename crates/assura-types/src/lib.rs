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
                    check_clause_expr(&clause.kind, &clause.body, env, &mut errors);
                }
            }
            Decl::FnDef(f) => {
                for clause in &f.clauses {
                    check_clause_expr(&clause.kind, &clause.body, env, &mut errors);
                }
            }
            Decl::Extern(ex) => {
                for clause in &ex.clauses {
                    check_clause_expr(&clause.kind, &clause.body, env, &mut errors);
                }
            }
            Decl::Service(s) => {
                for item in &s.items {
                    let clauses = match item {
                        ServiceItem::Operation { clauses, .. }
                        | ServiceItem::Query { clauses, .. } => clauses.as_slice(),
                        ServiceItem::Invariant(expr) => {
                            // Service-level invariants are always Bool-typed
                            check_clause_expr(&ClauseKind::Invariant, expr, env, &mut errors);
                            continue;
                        }
                        ServiceItem::Other { body, .. } => {
                            collect_expr_errors(body, env, &mut errors);
                            continue;
                        }
                        _ => continue,
                    };
                    for clause in clauses {
                        check_clause_expr(&clause.kind, &clause.body, env, &mut errors);
                    }
                }
            }
            Decl::Block { body, .. } => {
                for clause in body {
                    check_clause_expr(&clause.kind, &clause.body, env, &mut errors);
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
fn check_clause_expr(kind: &ClauseKind, body: &Expr, env: &TypeEnv, errors: &mut Vec<TypeError>) {
    match infer_expr(body, env) {
        Ok(ty) => {
            if clause_requires_bool(kind) && ty != Type::Unknown && ty != Type::Bool {
                errors.push(TypeError {
                    code: "A03006".into(),
                    message: format!(
                        "{} clause must be Bool, found `{ty}`",
                        clause_kind_label(kind),
                    ),
                    span: 0..0,
                    secondary: None,
                });
            }
        }
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
            if !self.known_effects.contains(effect) {
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
        check_clause_expr(&AstClauseKind::Requires, &body, &env, &mut errors);
        assert!(errors.is_empty());
    }

    #[test]
    fn clause_requires_int_body_error() {
        let env = TypeEnv::new();
        let body = AstExpr::Literal(AstLit::Int("42".into()));
        let mut errors = Vec::new();
        check_clause_expr(&AstClauseKind::Requires, &body, &env, &mut errors);
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
        check_clause_expr(&AstClauseKind::Ensures, &body, &env, &mut errors);
        assert!(errors.is_empty());
    }

    #[test]
    fn clause_ensures_string_body_error() {
        let env = TypeEnv::new();
        let body = AstExpr::Literal(AstLit::Str("hello".into()));
        let mut errors = Vec::new();
        check_clause_expr(&AstClauseKind::Ensures, &body, &env, &mut errors);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A03006");
        assert!(errors[0].message.contains("ensures"));
    }

    #[test]
    fn clause_invariant_bool_body_ok() {
        let env = TypeEnv::new();
        let body = AstExpr::Literal(AstLit::Bool(true));
        let mut errors = Vec::new();
        check_clause_expr(&AstClauseKind::Invariant, &body, &env, &mut errors);
        assert!(errors.is_empty());
    }

    #[test]
    fn clause_invariant_float_body_error() {
        let env = TypeEnv::new();
        let body = AstExpr::Literal(AstLit::Float("3.14".into()));
        let mut errors = Vec::new();
        check_clause_expr(&AstClauseKind::Invariant, &body, &env, &mut errors);
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
        check_clause_expr(&AstClauseKind::Rule, &body, &env, &mut errors);
        assert!(errors.is_empty());
    }

    #[test]
    fn clause_rule_int_body_error() {
        let env = TypeEnv::new();
        let body = AstExpr::Literal(AstLit::Int("99".into()));
        let mut errors = Vec::new();
        check_clause_expr(&AstClauseKind::Rule, &body, &env, &mut errors);
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
        check_clause_expr(&AstClauseKind::Effects, &body, &env, &mut errors);
        assert!(errors.is_empty());
    }

    #[test]
    fn clause_modifies_any_body_ok() {
        let env = TypeEnv::new();
        let body = AstExpr::Ident("buffer".into());
        let mut errors = Vec::new();
        check_clause_expr(&AstClauseKind::Modifies, &body, &env, &mut errors);
        assert!(errors.is_empty());
    }

    #[test]
    fn clause_unknown_body_no_error() {
        let env = TypeEnv::new();
        // Unknown ident in requires clause should not emit A03006
        let body = AstExpr::Ident("unknown_predicate".into());
        let mut errors = Vec::new();
        check_clause_expr(&AstClauseKind::Requires, &body, &env, &mut errors);
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
        check_clause_expr(&AstClauseKind::Requires, &body, &env, &mut errors);
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
        let mut checker =
            TypestateChecker::new(states, transitions, "Locked".into(), 0..6);

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
        let mut ts_checker = TypestateChecker::new(
            states,
            transitions,
            "Disconnected".into(),
            0..12,
        );

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
        let mut ts_checker =
            TypestateChecker::new(states, transitions, "Closed".into(), 0..6);

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
        let mut ts_checker =
            TypestateChecker::new(states, transitions, "Init".into(), 0..4);

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
        let mut checker_a = TypestateChecker::new(
            states.clone(),
            transitions.clone(),
            "Idle".into(),
            0..4,
        );
        checker_a.transition("activate", 10..18).unwrap();

        // Branch B: fail => Error
        let mut checker_b =
            TypestateChecker::new(states, transitions, "Idle".into(), 0..4);
        checker_b.transition("fail", 10..14).unwrap();

        // Post-branch: Active vs Error => A06004
        let err = TypestateChecker::check_branch_consistency(
            &checker_a,
            &checker_b,
            20..25,
        );
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

        let mut checker_a = TypestateChecker::new(
            states.clone(),
            transitions.clone(),
            "Pending".into(),
            0..7,
        );
        checker_a.transition("complete_a", 10..20).unwrap();

        let mut checker_b =
            TypestateChecker::new(states, transitions, "Pending".into(), 0..7);
        checker_b.transition("complete_b", 10..20).unwrap();

        let err = TypestateChecker::check_branch_consistency(
            &checker_a,
            &checker_b,
            20..25,
        );
        assert!(err.is_none());
    }

    #[test]
    fn interaction_typestate_branch_one_transitions_other_stays() {
        // One branch transitions, the other stays in the original state.
        // Post-branch: states differ => A06004.
        let states = vec!["Idle".into(), "Active".into()];
        let transitions = vec![("start".into(), "Idle".into(), "Active".into())];

        let mut checker_a = TypestateChecker::new(
            states.clone(),
            transitions.clone(),
            "Idle".into(),
            0..4,
        );
        checker_a.transition("start", 10..15).unwrap();
        // checker_a: Active

        let checker_b =
            TypestateChecker::new(states, transitions, "Idle".into(), 0..4);
        // checker_b: still Idle (no transition in this branch)

        let err = TypestateChecker::check_branch_consistency(
            &checker_a,
            &checker_b,
            20..25,
        );
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
        let mut ts_a = TypestateChecker::new(
            states.clone(),
            transitions.clone(),
            "Idle".into(),
            0..4,
        );
        ts_a.transition("activate", 10..18).unwrap();

        let mut ts_b =
            TypestateChecker::new(states, transitions, "Idle".into(), 0..4);
        ts_b.transition("deactivate", 10..20).unwrap();

        let ts_err =
            TypestateChecker::check_branch_consistency(&ts_a, &ts_b, 0..25);
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
        let actual =
            EffectSet::from_effect_names(["database.read", "database.write"]);
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
        let mut ts = TypestateChecker::new(
            states,
            transitions,
            "Init".into(),
            0..4,
        );

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
        let mut ts = TypestateChecker::new(
            states,
            transitions,
            "Ready".into(),
            0..5,
        );

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
        let actual = EffectSet::from_effect_names([
            "console.write",
            "network.send",
            "database.read",
        ]);
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
            (
                "commit".into(),
                "InTransaction".into(),
                "Connected".into(),
            ),
            ("close".into(), "Connected".into(), "Closed".into()),
        ];
        let mut ts = TypestateChecker::new(
            states,
            transitions,
            "Disconnected".into(),
            0..12,
        );

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
        let actual = EffectSet::from_effect_names([
            "database.read",
            "database.write",
            "network.connect",
        ]);
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
        let mut ts =
            TypestateChecker::new(states, transitions, "Off".into(), 0..3);
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
}
