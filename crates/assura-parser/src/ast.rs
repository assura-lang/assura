pub type Span = std::ops::Range<usize>;

#[derive(Debug, Clone)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

// ---------------------------------------------------------------------------
// Top-level file
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub project: Option<ProjectDecl>,
    pub module: Option<ModuleDecl>,
    pub imports: Vec<ImportDecl>,
    pub decls: Vec<Spanned<Decl>>,
}

#[derive(Debug, Clone)]
pub struct ProjectDecl {
    pub name: String,
    pub profile: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ModuleDecl {
    pub path: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ImportDecl {
    pub path: Vec<String>,
    pub alias: Option<String>,
    pub items: Vec<String>,
    pub span: Span,
}

// ---------------------------------------------------------------------------
// Declarations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Decl {
    Contract(ContractDecl),
    Service(ServiceDecl),
    TypeDef(TypeDef),
    EnumDef(EnumDef),
    Extern(ExternDecl),
    Bind(BindDecl),
    /// Ghost prophecy variable declaration
    Prophecy(ProphecyDecl),
    FnDef(FnDef),
    /// Codec registry declaration (FMT.4)
    CodecRegistry(CodecRegistryDecl),
    /// Catch-all for extended syntax (feature, incremental, liveness, etc.)
    Block {
        kind: BlockKind,
        name: String,
        /// Optional inline value (e.g., `feature_max X: Nat = 280` stores `["280"]`).
        value: Option<Vec<String>>,
        body: Vec<Clause>,
    },
}

/// The kind of a generic block declaration.
///
/// The parser's catch-all `Decl::Block` can represent many kinds of extended
/// syntax (features, axiomatic definitions, lock ordering, etc.). This enum
/// gives each known kind a strongly-typed variant instead of raw strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    /// `feature_max NAME: Nat = VALUE`
    FeatureMax,
    /// `feature NAME = VALUE`
    Feature,
    /// `axiomatic NAME { ... }` or `axiom NAME { ... }`
    Axiomatic,
    /// `lock_order { ... }` or `lock_hierarchy { ... }`
    LockOrder,
    /// `unsafe NAME { ... }` or `unsafe_escape NAME { ... }`
    UnsafeEscape,
    /// `liveness NAME { ... }`
    Liveness,
    /// `library NAME { ... }` or `package NAME { ... }`
    Library,
    /// `interface NAME { ... }`
    Interface,
    /// `table NAME { ... }`
    Table,
    /// `incremental NAME { ... }`
    Incremental,
    /// Unknown or user-defined block kind.
    Other(String),
}

impl BlockKind {
    /// Parse a block kind from its raw token text. Synonyms are normalized
    /// to their canonical variant (e.g. `"axiom"` -> `Axiomatic`).
    pub fn from_keyword(s: &str) -> Self {
        match s {
            "feature_max" => Self::FeatureMax,
            "feature" => Self::Feature,
            "axiomatic" | "axiom" => Self::Axiomatic,
            "lock_order" | "lock_hierarchy" => Self::LockOrder,
            "unsafe" | "unsafe_escape" => Self::UnsafeEscape,
            "liveness" => Self::Liveness,
            "library" | "package" => Self::Library,
            "interface" => Self::Interface,
            "table" => Self::Table,
            "incremental" => Self::Incremental,
            other => Self::Other(other.to_string()),
        }
    }
}

impl std::fmt::Display for BlockKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FeatureMax => write!(f, "feature_max"),
            Self::Feature => write!(f, "feature"),
            Self::Axiomatic => write!(f, "axiomatic"),
            Self::LockOrder => write!(f, "lock_order"),
            Self::UnsafeEscape => write!(f, "unsafe_escape"),
            Self::Liveness => write!(f, "liveness"),
            Self::Library => write!(f, "library"),
            Self::Interface => write!(f, "interface"),
            Self::Table => write!(f, "table"),
            Self::Incremental => write!(f, "incremental"),
            Self::Other(s) => write!(f, "{s}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ContractDecl {
    pub name: String,
    pub type_params: Vec<String>,
    pub clauses: Vec<Clause>,
    /// Parameter names from inline `fn` declarations inside the contract.
    /// These are in scope for clause bodies (requires, ensures, etc.).
    pub fn_params: Vec<Param>,
}

#[derive(Debug, Clone)]
pub struct Clause {
    pub kind: ClauseKind,
    pub body: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClauseKind {
    Requires,
    Ensures,
    Effects,
    Invariant,
    Modifies,
    Input,
    Output,
    Errors,
    Rule,
    DataFlow,
    MustNot,
    Decreases,
    /// Memory ordering annotation: `ordering: relaxed|acquire|release|acqrel|seq_cst`
    Ordering,
    Other(String),
}

/// Memory ordering for atomic operations (CONC.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryOrdering {
    Relaxed,
    Acquire,
    Release,
    AcqRel,
    SeqCst,
}

impl MemoryOrdering {
    /// Parse a string token into a `MemoryOrdering`, if valid.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "relaxed" => Some(Self::Relaxed),
            "acquire" => Some(Self::Acquire),
            "release" => Some(Self::Release),
            "acqrel" | "acq_rel" => Some(Self::AcqRel),
            "seq_cst" => Some(Self::SeqCst),
            _ => None,
        }
    }

    /// Returns the Rust `std::sync::atomic::Ordering` variant name.
    pub fn to_rust_ordering(&self) -> &'static str {
        match self {
            Self::Relaxed => "Relaxed",
            Self::Acquire => "Acquire",
            Self::Release => "Release",
            Self::AcqRel => "AcqRel",
            Self::SeqCst => "SeqCst",
        }
    }
}

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Expr {
    /// Integer, float, string, or boolean literal
    Literal(Literal),
    /// Named reference: variable, type, keyword-as-value
    Ident(String),
    /// Field access: `expr.field`
    Field(Box<Expr>, String),
    /// Method call: `expr.method(args)`
    MethodCall {
        receiver: Box<Expr>,
        method: String,
        args: Vec<Expr>,
    },
    /// Function call: `f(args)`
    Call { func: Box<Expr>, args: Vec<Expr> },
    /// Index access: `expr[index]`
    Index { expr: Box<Expr>, index: Box<Expr> },
    /// Binary operation
    BinOp {
        lhs: Box<Expr>,
        op: BinOp,
        rhs: Box<Expr>,
    },
    /// Unary operation
    UnaryOp { op: UnaryOp, expr: Box<Expr> },
    /// `old(expr)` for postconditions
    Old(Box<Expr>),
    /// `forall var in domain: body`
    Forall {
        var: String,
        domain: Box<Expr>,
        body: Box<Expr>,
    },
    /// `exists var in domain: body`
    Exists {
        var: String,
        domain: Box<Expr>,
        body: Box<Expr>,
    },
    /// `if cond then expr [else expr]`
    If {
        cond: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Option<Box<Expr>>,
    },
    /// Parenthesized expression
    Paren(Box<Expr>),
    /// List literal: `[a, b, c]`
    List(Vec<Expr>),
    /// Type cast: `expr as Type`
    Cast { expr: Box<Expr>, ty: String },
    /// Sequence of space-separated expressions (e.g., `pure incremental Foo`)
    Block(Vec<Expr>),
    /// Ghost block: verified but erased at runtime
    Ghost(Box<Expr>),
    /// Apply a lemma: `apply lemma_name(args)` — adds lemma ensures as assumption
    Apply { lemma_name: String, args: Vec<Expr> },
    /// Let binding: `let x = expr in body`
    Let {
        name: String,
        value: Box<Expr>,
        body: Box<Expr>,
    },
    /// Match expression: `match expr { pattern => body, ... }`
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
    },
    /// Tuple expression: `(a, b, c)`
    Tuple(Vec<Expr>),
    /// Unparsed token sequence (fallback)
    Raw(Vec<String>),
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Expr,
}

#[derive(Debug, Clone)]
pub enum Pattern {
    /// A simple identifier or enum variant name
    Ident(String),
    /// A literal value
    Literal(Literal),
    /// Wildcard pattern `_`
    Wildcard,
    /// Constructor pattern: `Variant(p1, p2)`
    Constructor { name: String, fields: Vec<Pattern> },
    /// Tuple pattern: `(a, b, c)`
    Tuple(Vec<Pattern>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Neq,
    Lt,
    Lte,
    Gt,
    Gte,
    And,
    Or,
    Implies,
    In,
    NotIn,
    Concat,
    Range,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Int(String),
    Float(String),
    Str(String),
    Bool(bool),
}

// ---------------------------------------------------------------------------
// Structured type expressions
// ---------------------------------------------------------------------------

/// A structured type expression, replacing `Vec<String>` raw tokens.
///
/// Produced by the type parser and consumed by type checking, HIR
/// lowering, and code generation without re-parsing.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    /// Simple named type: `Int`, `Bool`, `Foo`
    Named(String),
    /// Generic type application: `List<Int>`, `Map<String, Int>`
    Generic(String, Vec<TypeExpr>),
    /// Tuple type: `(Int, Bool)`
    Tuple(Vec<TypeExpr>),
    /// Function type: `fn(Int, Bool) -> String`
    Fn {
        params: Vec<TypeExpr>,
        ret: Box<TypeExpr>,
    },
    /// Refined type: `{ x: Int | x > 0 }`
    Refined {
        base: Box<TypeExpr>,
        /// The refinement predicate as raw token text.
        predicate: String,
    },
    /// Unit type (empty tuple)
    Unit,
}

impl TypeExpr {
    /// Convert to a simple string representation for display purposes.
    pub fn to_display_string(&self) -> String {
        match self {
            TypeExpr::Named(name) => name.clone(),
            TypeExpr::Generic(name, args) => {
                let args_s: Vec<String> = args.iter().map(|a| a.to_display_string()).collect();
                format!("{}<{}>", name, args_s.join(", "))
            }
            TypeExpr::Tuple(elems) => {
                let elems_s: Vec<String> = elems.iter().map(|e| e.to_display_string()).collect();
                format!("({})", elems_s.join(", "))
            }
            TypeExpr::Fn { params, ret } => {
                let params_s: Vec<String> = params.iter().map(|p| p.to_display_string()).collect();
                format!("fn({}) -> {}", params_s.join(", "), ret.to_display_string())
            }
            TypeExpr::Refined { base, .. } => {
                format!("{{ {} | ... }}", base.to_display_string())
            }
            TypeExpr::Unit => "()".to_string(),
        }
    }

    /// Convenience: create a Named type.
    pub fn named(s: impl Into<String>) -> Self {
        TypeExpr::Named(s.into())
    }

    /// Convenience: create a Generic type.
    pub fn generic(name: impl Into<String>, args: Vec<TypeExpr>) -> Self {
        TypeExpr::Generic(name.into(), args)
    }
}

/// Best-effort parse of raw type token strings into a structured `TypeExpr`.
///
/// Returns `None` only for empty slices that cannot be interpreted.
pub(crate) fn try_parse_type_tokens(tokens: &[String]) -> Option<TypeExpr> {
    if tokens.is_empty() {
        return Some(TypeExpr::Unit);
    }
    if tokens.len() == 1 {
        return Some(TypeExpr::Named(tokens[0].clone()));
    }
    // Simple generic: Name<Arg1, Arg2>
    if tokens.len() >= 4 && tokens[1] == "<" && tokens.last().map(|s| s.as_str()) == Some(">") {
        let name = tokens[0].clone();
        let inner = &tokens[2..tokens.len() - 1];
        let args: Vec<TypeExpr> = inner
            .split(|t| t == ",")
            .filter(|s| !s.is_empty())
            .filter_map(try_parse_type_tokens)
            .collect();
        if !args.is_empty() {
            return Some(TypeExpr::Generic(name, args));
        }
    }
    // Fallback: join as named type
    Some(TypeExpr::Named(tokens.join(" ")))
}

// ---------------------------------------------------------------------------
// Shared clause parameter extraction
// ---------------------------------------------------------------------------

/// A parsed parameter from an input/output clause.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedParam {
    pub name: String,
    /// Raw type tokens (e.g., `["List", "<", "Int", ">"]`). Empty if untyped.
    pub ty: Vec<String>,
    /// Structured type expression parsed from `ty` tokens (if parseable).
    pub parsed_type: Option<TypeExpr>,
}

/// Extract `(name, type)` parameter pairs from a clause body expression.
///
/// Handles all known patterns:
/// - `input(a: Int, b: Bool)` -> Call with Cast args
/// - `input(x)` -> Call with Ident args (untyped)
/// - `input { a: Int }` -> Block with Cast elements
/// - Raw token fallback: `["a", ":", "Int", ",", "b", ":", "Bool"]`
pub fn extract_clause_params(body: &Expr) -> Vec<ParsedParam> {
    let mut params = Vec::new();
    extract_clause_params_inner(body, &mut params);
    params
}

fn extract_clause_params_inner(body: &Expr, params: &mut Vec<ParsedParam>) {
    match body {
        Expr::Call { args, .. } => {
            for arg in args {
                extract_single_param(arg, params);
            }
        }
        Expr::Cast { expr: inner, ty } => {
            if let Expr::Ident(name) = inner.as_ref() {
                let ty = vec![ty.clone()];
                let parsed_type = try_parse_type_tokens(&ty);
                params.push(ParsedParam {
                    name: name.clone(),
                    ty,
                    parsed_type,
                });
            }
        }
        Expr::Ident(name) => {
            params.push(ParsedParam {
                name: name.clone(),
                ty: vec![],
                parsed_type: None,
            });
        }
        Expr::Paren(inner) => extract_clause_params_inner(inner, params),
        Expr::Tuple(items) | Expr::Block(items) => {
            for item in items {
                extract_single_param(item, params);
            }
        }
        Expr::Raw(tokens) => extract_clause_params_from_raw(tokens, params),
        _ => {}
    }
}

fn extract_single_param(expr: &Expr, params: &mut Vec<ParsedParam>) {
    match expr {
        Expr::Cast { expr: inner, ty } => {
            if let Expr::Ident(name) = inner.as_ref() {
                let ty = vec![ty.clone()];
                let parsed_type = try_parse_type_tokens(&ty);
                params.push(ParsedParam {
                    name: name.clone(),
                    ty,
                    parsed_type,
                });
            }
        }
        Expr::Ident(name) => {
            params.push(ParsedParam {
                name: name.clone(),
                ty: vec![],
                parsed_type: None,
            });
        }
        Expr::Paren(inner) => extract_single_param(inner, params),
        _ => {}
    }
}

fn extract_clause_params_from_raw(tokens: &[String], params: &mut Vec<ParsedParam>) {
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i] == "," {
            i += 1;
            continue;
        }
        let sep = tokens.get(i + 1).map(|s| s.as_str());
        if matches!(sep, Some(":" | "as")) && i + 2 < tokens.len() {
            let name = tokens[i].clone();
            let type_start = i + 2;
            let mut j = type_start;
            let mut angle = 0i32;
            let mut brace = 0i32;
            let mut paren = 0i32;
            while j < tokens.len() {
                match tokens[j].as_str() {
                    // Only treat <> as angle brackets outside braces;
                    // inside { ... } (refinement types), < and > are
                    // comparison operators, not generic delimiters.
                    "<" if brace == 0 => angle += 1,
                    ">" if brace == 0 && angle > 0 => angle -= 1,
                    "{" => brace += 1,
                    "}" if brace > 0 => brace -= 1,
                    "(" => paren += 1,
                    ")" if paren > 0 => paren -= 1,
                    "," if angle == 0 && brace == 0 && paren == 0 => break,
                    _ => {}
                }
                j += 1;
            }
            let ty: Vec<String> = tokens[type_start..j].to_vec();
            let parsed_type = try_parse_type_tokens(&ty);
            params.push(ParsedParam {
                name,
                ty,
                parsed_type,
            });
            i = j;
        } else {
            // Bare identifier without type annotation
            let name = tokens[i].clone();
            if !name.is_empty()
                && name
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_')
            {
                params.push(ParsedParam {
                    name,
                    ty: vec![],
                    parsed_type: None,
                });
            }
            i += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Type / Enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TypeDef {
    pub name: String,
    pub type_params: Vec<String>,
    pub body: TypeBody,
}

#[derive(Debug, Clone)]
pub enum TypeBody {
    Alias(Vec<String>),
    Struct(Vec<FieldDef>),
    Refined(Vec<String>),
    Empty,
}

#[derive(Debug, Clone)]
pub struct FieldDef {
    pub name: String,
    pub ty: Vec<String>,
    /// Structured type expression parsed from `ty` tokens (if parseable).
    pub parsed_type: Option<TypeExpr>,
    pub is_pub: bool,
}

#[derive(Debug, Clone)]
pub struct EnumDef {
    pub name: String,
    pub type_params: Vec<String>,
    pub variants: Vec<EnumVariant>,
}

#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: String,
    pub fields: Vec<String>,
}

// ---------------------------------------------------------------------------
// Extern / Fn
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ExternDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub return_ty: Vec<String>,
    pub return_type_expr: Option<TypeExpr>,
    pub clauses: Vec<Clause>,
}

/// A `bind` declaration that maps a contract name to an existing Rust function path.
///
/// ```assura
/// bind "app::renderer::render_page" as render_page {
///     input(template: String, user: User)
///     output(result: Html)
///     requires { template.length > 0 }
///     ensures  { result.contains(user.name) }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct BindDecl {
    /// The contract name (the `as Ident` part).
    pub name: String,
    /// The Rust function path being bound (the string literal).
    pub target_path: String,
    pub params: Vec<Param>,
    pub return_ty: Vec<String>,
    pub return_type_expr: Option<TypeExpr>,
    pub clauses: Vec<Clause>,
}

/// Ghost prophecy variable: `ghost prophecy <name>: <type>`
#[derive(Debug, Clone)]
pub struct ProphecyDecl {
    pub name: String,
    pub ty_tokens: Vec<String>,
}

// ---------------------------------------------------------------------------
// Codec Registry (FMT.4)
// ---------------------------------------------------------------------------

/// A codec registry declaration: `codec_registry <name> { output: <type>, codec ... }`
#[derive(Debug, Clone)]
pub struct CodecRegistryDecl {
    pub name: String,
    /// The common output type (e.g., `ImageOutput`)
    pub output_type: Vec<String>,
    /// Individual codec entries
    pub codecs: Vec<CodecEntry>,
}

/// A single codec in a registry: `codec <name> { magic: [...], decoder: <fn>, contracts: { ... } }`
#[derive(Debug, Clone)]
pub struct CodecEntry {
    pub name: String,
    /// Magic byte pattern for identification
    pub magic: MagicPattern,
    /// Decoder function name
    pub decoder: String,
    /// Per-codec contract clauses
    pub contracts: Vec<Clause>,
}

/// The way a codec identifies its format
#[derive(Debug, Clone)]
pub enum MagicPattern {
    /// Exact or prefix byte pattern: `[0x89, 0x50, ...]` or `[0xFF, 0xD8, ..]`
    Bytes { bytes: Vec<u8>, prefix: bool },
    /// File extension matching: `extension("png", "apng")`
    Extension(Vec<String>),
    /// Probe function: `probe(is_tga)`
    Probe(String),
}

#[derive(Debug, Clone)]
pub struct FnDef {
    pub name: String,
    pub is_ghost: bool,
    pub is_lemma: bool,
    pub params: Vec<Param>,
    pub return_ty: Vec<String>,
    pub return_type_expr: Option<TypeExpr>,
    pub clauses: Vec<Clause>,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: Vec<String>,
    /// Structured type expression parsed from `ty` tokens (if parseable).
    pub parsed_type: Option<TypeExpr>,
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ServiceDecl {
    pub name: String,
    pub items: Vec<ServiceItem>,
}

#[derive(Debug, Clone)]
pub enum ServiceItem {
    TypeDef(TypeDef),
    EnumDef(EnumDef),
    States(Vec<String>),
    Operation { name: String, clauses: Vec<Clause> },
    Query { name: String, clauses: Vec<Clause> },
    Invariant(Expr),
    Other { kind: String, body: Expr },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_params_refined_type_with_less_than() {
        // a : { x : Int | x < 10 }, b : Bool
        // The `<` inside the refinement must NOT be treated as an angle bracket.
        let tokens: Vec<String> = vec![
            "a", ":", "{", "x", ":", "Int", "|", "x", "<", "10", "}", ",", "b", ":", "Bool",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let body = Expr::Raw(tokens);
        let params = extract_clause_params(&body);
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "a");
        assert_eq!(
            params[0].ty,
            vec!["{", "x", ":", "Int", "|", "x", "<", "10", "}"]
        );
        assert_eq!(params[1].name, "b");
        assert_eq!(params[1].ty, vec!["Bool"]);
    }

    #[test]
    fn extract_params_refined_type_with_parens() {
        // val : ( Int , Bool )
        let tokens: Vec<String> = vec!["val", ":", "(", "Int", ",", "Bool", ")"]
            .into_iter()
            .map(String::from)
            .collect();
        let body = Expr::Raw(tokens);
        let params = extract_clause_params(&body);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "val");
        assert_eq!(params[0].ty, vec!["(", "Int", ",", "Bool", ")"]);
    }

    #[test]
    fn extract_params_generic_type() {
        // a : List < Int >, b : Map < String , Int >
        let tokens: Vec<String> = vec![
            "a", ":", "List", "<", "Int", ">", ",", "b", ":", "Map", "<", "String", ",", "Int", ">",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let body = Expr::Raw(tokens);
        let params = extract_clause_params(&body);
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "a");
        assert_eq!(params[0].ty, vec!["List", "<", "Int", ">"]);
        assert_eq!(params[1].name, "b");
        assert_eq!(params[1].ty, vec!["Map", "<", "String", ",", "Int", ">"]);
    }
}
