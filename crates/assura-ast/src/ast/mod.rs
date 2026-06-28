//! Assura AST types (canonical compiler IR).
//!
//! Kept as a single module body for type ordering (Expr/SpExpr/visitors).
//! Large sections are delimited by comments; further physical splits need
//! a dependency DAG (Spanned -> Expr -> SpExpr -> clauses/decls).

pub type Span = std::ops::Range<usize>;

#[derive(Debug, Clone, PartialEq)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    /// Create a `Spanned` with a zero-length sentinel span.
    /// Useful in tests where source locations are irrelevant.
    pub fn no_span(node: T) -> Self {
        Self { node, span: 0..0 }
    }
}

/// Shorthand for `Spanned<Expr>`. Used throughout the AST wherever
/// an expression with source location is needed.
pub type SpExpr = Spanned<Expr>;

// ---------------------------------------------------------------------------
// Top-level file
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct SourceFile {
    pub project: Option<ProjectDecl>,
    pub module: Option<ModuleDecl>,
    pub imports: Vec<ImportDecl>,
    pub decls: Vec<Spanned<Decl>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectDecl {
    pub name: String,
    pub profile: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModuleDecl {
    pub path: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportDecl {
    pub path: Vec<String>,
    pub alias: Option<String>,
    pub items: Vec<String>,
    pub span: Span,
}

// ---------------------------------------------------------------------------
// Declarations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
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
// Decl accessors
// ---------------------------------------------------------------------------

impl Decl {
    /// Returns the declaration name, if it has one.
    pub fn name(&self) -> Option<&str> {
        match self {
            Decl::Contract(c) => Some(&c.name),
            Decl::Service(s) => Some(&s.name),
            Decl::TypeDef(t) => Some(&t.name),
            Decl::EnumDef(e) => Some(&e.name),
            Decl::Extern(e) => Some(&e.name),
            Decl::Bind(b) => Some(&b.name),
            Decl::Prophecy(p) => Some(&p.name),
            Decl::FnDef(f) => Some(&f.name),
            Decl::CodecRegistry(r) => Some(&r.name),
            Decl::Block { name, .. } => Some(name),
        }
    }

    /// Returns the clauses of the declaration, if it has any.
    pub fn clauses(&self) -> &[Clause] {
        match self {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Extern(e) => &e.clauses,
            Decl::Bind(b) => &b.clauses,
            Decl::Service(_) => &[],
            Decl::Block { body, .. } => body,
            Decl::TypeDef(_) | Decl::EnumDef(_) | Decl::Prophecy(_) | Decl::CodecRegistry(_) => &[],
        }
    }

    /// Human-readable kind + name label, e.g. `"contract SafeDiv"`, `"fn foo"`.
    ///
    /// Useful for pipeline/MCP declaration lists and debug output.
    pub fn summary_label(&self) -> String {
        match self {
            Decl::Contract(c) => format!("contract {}", c.name),
            Decl::Bind(b) => format!("bind {}", b.name),
            Decl::FnDef(f) => format!("fn {}", f.name),
            Decl::Service(s) => format!("service {}", s.name),
            Decl::TypeDef(t) => format!("type {}", t.name),
            Decl::EnumDef(e) => format!("enum {}", e.name),
            Decl::Extern(e) => format!("extern {}", e.name),
            Decl::Prophecy(p) => format!("prophecy {}", p.name),
            Decl::CodecRegistry(c) => format!("codec_registry {}", c.name),
            Decl::Block { kind, name, .. } => format!("{kind} {name}"),
        }
    }

    /// Returns the parameters, if the declaration has them.
    pub fn params(&self) -> &[Param] {
        match self {
            Decl::Contract(c) => &c.fn_params,
            Decl::FnDef(f) => &f.params,
            Decl::Extern(e) => &e.params,
            Decl::Bind(b) => &b.params,
            _ => &[],
        }
    }

    /// Returns `true` if this is a ghost or lemma function (no runtime semantics).
    pub fn is_ghost_or_lemma(&self) -> bool {
        matches!(self, Decl::FnDef(f) if f.is_ghost || f.is_lemma)
    }

    /// Returns the return type expression, if the declaration has one.
    pub fn return_ty(&self) -> Option<&TypeExpr> {
        match self {
            Decl::FnDef(f) => f.return_ty.as_ref(),
            Decl::Extern(e) => e.return_ty.as_ref(),
            Decl::Bind(b) => b.return_ty.as_ref(),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct ContractDecl {
    pub name: String,
    pub type_params: Vec<String>,
    pub clauses: Vec<Clause>,
    /// Parameter names from inline `fn` declarations inside the contract.
    /// These are in scope for clause bodies (requires, ensures, etc.).
    pub fn_params: Vec<Param>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Clause {
    pub kind: ClauseKind,
    pub body: SpExpr,
    /// Effect row variables (e.g., `E` in `effects <io | E>`).
    /// Only populated for `ClauseKind::Effects` clauses.
    pub effect_variables: Vec<String>,
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

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Integer, float, string, or boolean literal
    Literal(Literal),
    /// Named reference: variable, type, keyword-as-value
    Ident(String),
    /// Field access: `expr.field`
    Field(Box<SpExpr>, String),
    /// Method call: `expr.method(args)`
    MethodCall {
        receiver: Box<SpExpr>,
        method: String,
        args: Vec<SpExpr>,
    },
    /// Function call: `f(args)`
    Call {
        func: Box<SpExpr>,
        args: Vec<SpExpr>,
    },
    /// Index access: `expr[index]`
    Index {
        expr: Box<SpExpr>,
        index: Box<SpExpr>,
    },
    /// Binary operation
    BinOp {
        lhs: Box<SpExpr>,
        op: BinOp,
        rhs: Box<SpExpr>,
    },
    /// Unary operation
    UnaryOp { op: UnaryOp, expr: Box<SpExpr> },
    /// `old(expr)` for postconditions
    Old(Box<SpExpr>),
    /// `forall var in domain: body`
    Forall {
        var: String,
        domain: Box<SpExpr>,
        body: Box<SpExpr>,
    },
    /// `exists var in domain: body`
    Exists {
        var: String,
        domain: Box<SpExpr>,
        body: Box<SpExpr>,
    },
    /// `if cond then expr [else expr]`
    If {
        cond: Box<SpExpr>,
        then_branch: Box<SpExpr>,
        else_branch: Option<Box<SpExpr>>,
    },
    /// List literal: `[a, b, c]`
    List(Vec<SpExpr>),
    /// Type cast: `expr as Type`
    Cast { expr: Box<SpExpr>, ty: String },
    /// Sequence of space-separated expressions (e.g., `pure incremental Foo`)
    Block(Vec<SpExpr>),
    /// Ghost block: verified but erased at runtime
    Ghost(Box<SpExpr>),
    /// Apply a lemma: `apply lemma_name(args)` — adds lemma ensures as assumption
    Apply {
        lemma_name: String,
        args: Vec<SpExpr>,
    },
    /// Let binding: `let x = expr in body`
    Let {
        name: String,
        value: Box<SpExpr>,
        body: Box<SpExpr>,
    },
    /// Match expression: `match expr { pattern => body, ... }`
    Match {
        scrutinee: Box<SpExpr>,
        arms: Vec<MatchArm>,
    },
    /// Tuple expression: `(a, b, c)`
    Tuple(Vec<SpExpr>),
    /// Unparsed token sequence (fallback)
    Raw(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: SpExpr,
}

#[derive(Debug, Clone, PartialEq)]
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

impl BinOp {
    /// Returns a triple of (assura_source, rust_source, ident) representations.
    /// This centralizes the per-operator data to avoid duplicating the 17-arm
    /// match in three different methods.
    fn repr(&self) -> (&'static str, &'static str, &'static str) {
        match self {
            BinOp::Add => ("+", "+", "add"),
            BinOp::Sub => ("-", "-", "sub"),
            BinOp::Mul => ("*", "*", "mul"),
            BinOp::Div => ("/", "/", "div"),
            BinOp::Mod => ("mod", "%", "mod"),
            BinOp::Eq => ("==", "==", "eq"),
            BinOp::Neq => ("!=", "!=", "neq"),
            BinOp::Lt => ("<", "<", "lt"),
            BinOp::Lte => ("<=", "<=", "lte"),
            BinOp::Gt => (">", ">", "gt"),
            BinOp::Gte => (">=", ">=", "gte"),
            BinOp::And => ("and", "&&", "and"),
            BinOp::Or => ("or", "||", "or"),
            BinOp::Implies => ("=>", "/* implies */", "implies"),
            BinOp::In => ("in", "/* in */", "in"),
            BinOp::NotIn => ("not in", "/* not in */", "notin"),
            BinOp::Concat => ("++", "/* ++ */", "concat"),
            BinOp::Range => ("..", "..", "range"),
        }
    }

    /// Returns the Assura source-level string for this operator.
    pub fn as_str(&self) -> &'static str {
        self.repr().0
    }

    /// Returns an identifier-safe name for this operator (e.g., "add", "sub").
    pub fn as_ident(&self) -> &'static str {
        self.repr().2
    }

    /// Returns the Rust code-level string for this operator.
    pub fn as_rust_str(&self) -> &'static str {
        self.repr().1
    }

    /// Returns true for arithmetic operators (+ - * / %).
    /// These typically require numeric operands.
    pub fn is_arithmetic(&self) -> bool {
        matches!(
            self,
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod
        )
    }

    /// Returns true for all comparison/relational operators (== != < <= > >=).
    pub fn is_comparison(&self) -> bool {
        matches!(
            self,
            BinOp::Eq | BinOp::Neq | BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte
        )
    }

    /// Returns true for ordering comparisons (< <= > >=).
    /// These are the ones that often need i128 widening casts for mixed-width
    /// numeric comparisons in generated Rust.
    pub fn is_ordering_comparison(&self) -> bool {
        matches!(self, BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte)
    }

    /// Returns true for logical operators (and, or, implies).
    pub fn is_logical(&self) -> bool {
        matches!(self, BinOp::And | BinOp::Or | BinOp::Implies)
    }

    /// Returns true for division and modulo (special handling for div-by-zero).
    pub fn is_division_like(&self) -> bool {
        matches!(self, BinOp::Div | BinOp::Mod)
    }

    /// Returns true for collection membership tests (in, not in).
    pub fn is_membership(&self) -> bool {
        matches!(self, BinOp::In | BinOp::NotIn)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    Neg,
    Not,
}

impl UnaryOp {
    /// Assura source string for the operator.
    pub fn as_str(&self) -> &'static str {
        match self {
            UnaryOp::Neg => "-",
            UnaryOp::Not => "not",
        }
    }

    /// Rust source string for the operator.
    pub fn as_rust_str(&self) -> &'static str {
        match self {
            UnaryOp::Neg => "-",
            UnaryOp::Not => "!",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Int(String),
    Float(String),
    Str(String),
    Bool(bool),
}

// ---------------------------------------------------------------------------
// Declaration visitor trait
// ---------------------------------------------------------------------------

/// Visitor trait for walking top-level `Decl` nodes. Each `visit_*` method has
/// a default no-op (or clause-walking) implementation. Override only the
/// variants you care about.
///
/// Prefer this over open-coding `match &decl.node { Decl::Contract ... }` in
/// every pass that only needs a subset of declarations (codegen name collection,
/// span maps, MCP declaration lists, etc.).
pub trait DeclVisitor {
    /// Called for every declaration. Default dispatches to variant methods.
    fn visit_decl(&mut self, decl: &Decl) {
        walk_decl(self, decl);
    }

    fn visit_contract(&mut self, _c: &ContractDecl) {}
    fn visit_service(&mut self, _s: &ServiceDecl) {}
    fn visit_type_def(&mut self, _t: &TypeDef) {}
    fn visit_enum_def(&mut self, _e: &EnumDef) {}
    fn visit_extern(&mut self, _e: &ExternDecl) {}
    fn visit_bind(&mut self, _b: &BindDecl) {}
    fn visit_prophecy(&mut self, _p: &ProphecyDecl) {}
    fn visit_fn_def(&mut self, _f: &FnDef) {}
    fn visit_codec_registry(&mut self, _c: &CodecRegistryDecl) {}
    fn visit_block(
        &mut self,
        _kind: &BlockKind,
        _name: &str,
        _value: &Option<Vec<String>>,
        _body: &[Clause],
    ) {
    }
}

/// Walk a `Decl`, dispatching to the appropriate `visit_*` method.
pub fn walk_decl(visitor: &mut (impl DeclVisitor + ?Sized), decl: &Decl) {
    match decl {
        Decl::Contract(c) => visitor.visit_contract(c),
        Decl::Service(s) => visitor.visit_service(s),
        Decl::TypeDef(t) => visitor.visit_type_def(t),
        Decl::EnumDef(e) => visitor.visit_enum_def(e),
        Decl::Extern(e) => visitor.visit_extern(e),
        Decl::Bind(b) => visitor.visit_bind(b),
        Decl::Prophecy(p) => visitor.visit_prophecy(p),
        Decl::FnDef(f) => visitor.visit_fn_def(f),
        Decl::CodecRegistry(c) => visitor.visit_codec_registry(c),
        Decl::Block {
            kind,
            name,
            value,
            body,
        } => visitor.visit_block(kind, name, value, body),
    }
}

/// Walk all declarations in a source file.
pub fn walk_decls(visitor: &mut (impl DeclVisitor + ?Sized), decls: &[Spanned<Decl>]) {
    for d in decls {
        visitor.visit_decl(&d.node);
    }
}

/// Value-producing walker over `Decl` nodes. Unlike `DeclVisitor` (side-effecting),
/// `DeclFolder` returns an `Output` for every declaration.
pub trait DeclFolder {
    type Output;

    fn fold_decl(&mut self, decl: &Decl) -> Self::Output {
        match decl {
            Decl::Contract(c) => self.fold_contract(c),
            Decl::Service(s) => self.fold_service(s),
            Decl::TypeDef(t) => self.fold_type_def(t),
            Decl::EnumDef(e) => self.fold_enum_def(e),
            Decl::Extern(e) => self.fold_extern(e),
            Decl::Bind(b) => self.fold_bind(b),
            Decl::Prophecy(p) => self.fold_prophecy(p),
            Decl::FnDef(f) => self.fold_fn_def(f),
            Decl::CodecRegistry(c) => self.fold_codec_registry(c),
            Decl::Block {
                kind,
                name,
                value,
                body,
            } => self.fold_block(kind, name, value, body),
        }
    }

    fn fold_contract(&mut self, c: &ContractDecl) -> Self::Output;
    fn fold_service(&mut self, s: &ServiceDecl) -> Self::Output;
    fn fold_type_def(&mut self, t: &TypeDef) -> Self::Output;
    fn fold_enum_def(&mut self, e: &EnumDef) -> Self::Output;
    fn fold_extern(&mut self, e: &ExternDecl) -> Self::Output;
    fn fold_bind(&mut self, b: &BindDecl) -> Self::Output;
    fn fold_prophecy(&mut self, p: &ProphecyDecl) -> Self::Output;
    fn fold_fn_def(&mut self, f: &FnDef) -> Self::Output;
    fn fold_codec_registry(&mut self, c: &CodecRegistryDecl) -> Self::Output;
    fn fold_block(
        &mut self,
        kind: &BlockKind,
        name: &str,
        value: &Option<Vec<String>>,
        body: &[Clause],
    ) -> Self::Output;
}

// ---------------------------------------------------------------------------
// Expression visitor trait
// ---------------------------------------------------------------------------

/// Visitor trait for walking `Expr` trees. Each `visit_*` method has a default
/// that recurses into sub-expressions via `walk_expr`. Override only the
/// methods you care about; the default traversal handles the rest.
///
/// All methods receive `&SpExpr` (`Spanned<Expr>`) so implementations can
/// access source spans. The dispatch in `walk_expr` matches on `.node`.
pub trait ExprVisitor {
    /// Called for every expression node before dispatching to variant-specific
    /// methods. Override for pre/post-order hooks on all expressions.
    fn visit_expr(&mut self, expr: &SpExpr) {
        walk_expr(self, expr);
    }
    fn visit_literal(&mut self, _lit: &Literal) {}
    fn visit_ident(&mut self, _name: &str) {}
    fn visit_field(&mut self, base: &SpExpr, _field: &str) {
        self.visit_expr(base);
    }
    fn visit_method_call(&mut self, receiver: &SpExpr, _method: &str, args: &[SpExpr]) {
        self.visit_expr(receiver);
        for arg in args {
            self.visit_expr(arg);
        }
    }
    fn visit_call(&mut self, func: &SpExpr, args: &[SpExpr]) {
        self.visit_expr(func);
        for arg in args {
            self.visit_expr(arg);
        }
    }
    fn visit_index(&mut self, base: &SpExpr, index: &SpExpr) {
        self.visit_expr(base);
        self.visit_expr(index);
    }
    fn visit_binop(&mut self, lhs: &SpExpr, _op: &BinOp, rhs: &SpExpr) {
        self.visit_expr(lhs);
        self.visit_expr(rhs);
    }
    fn visit_unary_op(&mut self, _op: &UnaryOp, inner: &SpExpr) {
        self.visit_expr(inner);
    }
    fn visit_old(&mut self, inner: &SpExpr) {
        self.visit_expr(inner);
    }
    fn visit_forall(&mut self, _var: &str, domain: &SpExpr, body: &SpExpr) {
        self.visit_expr(domain);
        self.visit_expr(body);
    }
    fn visit_exists(&mut self, _var: &str, domain: &SpExpr, body: &SpExpr) {
        self.visit_expr(domain);
        self.visit_expr(body);
    }
    fn visit_if(&mut self, cond: &SpExpr, then_br: &SpExpr, else_br: Option<&SpExpr>) {
        self.visit_expr(cond);
        self.visit_expr(then_br);
        if let Some(e) = else_br {
            self.visit_expr(e);
        }
    }
    fn visit_list(&mut self, items: &[SpExpr]) {
        for item in items {
            self.visit_expr(item);
        }
    }
    fn visit_cast(&mut self, inner: &SpExpr, _ty: &str) {
        self.visit_expr(inner);
    }
    fn visit_block(&mut self, exprs: &[SpExpr]) {
        for e in exprs {
            self.visit_expr(e);
        }
    }
    fn visit_ghost(&mut self, inner: &SpExpr) {
        self.visit_expr(inner);
    }
    fn visit_apply(&mut self, _name: &str, args: &[SpExpr]) {
        for arg in args {
            self.visit_expr(arg);
        }
    }
    fn visit_let(&mut self, _name: &str, value: &SpExpr, body: &SpExpr) {
        self.visit_expr(value);
        self.visit_expr(body);
    }
    fn visit_match(&mut self, scrutinee: &SpExpr, arms: &[MatchArm]) {
        self.visit_expr(scrutinee);
        for arm in arms {
            self.visit_expr(&arm.body);
        }
    }
    fn visit_tuple(&mut self, items: &[SpExpr]) {
        for item in items {
            self.visit_expr(item);
        }
    }
    fn visit_raw(&mut self, _tokens: &[String]) {}
}

/// Walk a `SpExpr`, dispatching to the appropriate `visit_*` method on the
/// visitor. Called by the default `visit_expr` implementation.
pub fn walk_expr(visitor: &mut (impl ExprVisitor + ?Sized), expr: &SpExpr) {
    match &expr.node {
        Expr::Literal(lit) => visitor.visit_literal(lit),
        Expr::Ident(name) => visitor.visit_ident(name),
        Expr::Field(base, field) => visitor.visit_field(base, field),
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => visitor.visit_method_call(receiver, method, args),
        Expr::Call { func, args } => visitor.visit_call(func, args),
        Expr::Index { expr, index } => visitor.visit_index(expr, index),
        Expr::BinOp { lhs, op, rhs } => visitor.visit_binop(lhs, op, rhs),
        Expr::UnaryOp { op, expr } => visitor.visit_unary_op(op, expr),
        Expr::Old(inner) => visitor.visit_old(inner),
        Expr::Forall { var, domain, body } => visitor.visit_forall(var, domain, body),
        Expr::Exists { var, domain, body } => visitor.visit_exists(var, domain, body),
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => visitor.visit_if(cond, then_branch, else_branch.as_deref()),
        Expr::List(items) => visitor.visit_list(items),
        Expr::Cast { expr, ty } => visitor.visit_cast(expr, ty),
        Expr::Block(exprs) => visitor.visit_block(exprs),
        Expr::Ghost(inner) => visitor.visit_ghost(inner),
        Expr::Apply { lemma_name, args } => visitor.visit_apply(lemma_name, args),
        Expr::Let { name, value, body } => visitor.visit_let(name, value, body),
        Expr::Match { scrutinee, arms } => visitor.visit_match(scrutinee, arms),
        Expr::Tuple(items) => visitor.visit_tuple(items),
        Expr::Raw(tokens) => visitor.visit_raw(tokens),
    }
}

// ---------------------------------------------------------------------------
// Expression folder trait (value-producing walker)
// ---------------------------------------------------------------------------

/// A value-producing walker over `Expr` trees. Unlike `ExprVisitor` (which is
/// side-effecting), `ExprFolder` returns an `Output` for every expression node.
///
/// The default `fold_expr` dispatches to per-variant methods. Override any
/// method to customize behavior; call `self.fold_expr(sub)` to recurse.
///
/// All methods receive `&SpExpr` so implementations can access source spans.
pub trait ExprFolder {
    type Output;

    fn fold_expr(&mut self, expr: &SpExpr) -> Self::Output {
        match &expr.node {
            Expr::Literal(lit) => self.fold_literal(lit),
            Expr::Ident(s) => self.fold_ident(s),
            Expr::Field(base, field) => self.fold_field(base, field),
            Expr::MethodCall {
                receiver,
                method,
                args,
            } => self.fold_method_call(receiver, method, args),
            Expr::Call { func, args } => self.fold_call(func, args),
            Expr::Index { expr, index } => self.fold_index(expr, index),
            Expr::BinOp { lhs, op, rhs } => self.fold_binop(lhs, op, rhs),
            Expr::UnaryOp { op, expr } => self.fold_unary_op(op, expr),
            Expr::Old(inner) => self.fold_old(inner),
            Expr::Forall { var, domain, body } => self.fold_forall(var, domain, body),
            Expr::Exists { var, domain, body } => self.fold_exists(var, domain, body),
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => self.fold_if(cond, then_branch, else_branch.as_deref()),
            Expr::List(items) => self.fold_list(items),
            Expr::Cast { expr, ty } => self.fold_cast(expr, ty),
            Expr::Block(exprs) => self.fold_block(exprs),
            Expr::Ghost(inner) => self.fold_ghost(inner),
            Expr::Apply { lemma_name, args } => self.fold_apply(lemma_name, args),
            Expr::Let { name, value, body } => self.fold_let(name, value, body),
            Expr::Match { scrutinee, arms } => self.fold_match(scrutinee, arms),
            Expr::Tuple(items) => self.fold_tuple(items),
            Expr::Raw(tokens) => self.fold_raw(tokens),
        }
    }

    fn fold_literal(&mut self, lit: &Literal) -> Self::Output;
    fn fold_ident(&mut self, name: &str) -> Self::Output;
    fn fold_field(&mut self, base: &SpExpr, field: &str) -> Self::Output;
    fn fold_method_call(
        &mut self,
        receiver: &SpExpr,
        method: &str,
        args: &[SpExpr],
    ) -> Self::Output;
    fn fold_call(&mut self, func: &SpExpr, args: &[SpExpr]) -> Self::Output;
    fn fold_index(&mut self, base: &SpExpr, index: &SpExpr) -> Self::Output;
    fn fold_binop(&mut self, lhs: &SpExpr, op: &BinOp, rhs: &SpExpr) -> Self::Output;
    fn fold_unary_op(&mut self, op: &UnaryOp, inner: &SpExpr) -> Self::Output;
    fn fold_old(&mut self, inner: &SpExpr) -> Self::Output;
    fn fold_forall(&mut self, var: &str, domain: &SpExpr, body: &SpExpr) -> Self::Output;
    fn fold_exists(&mut self, var: &str, domain: &SpExpr, body: &SpExpr) -> Self::Output;
    fn fold_if(
        &mut self,
        cond: &SpExpr,
        then_br: &SpExpr,
        else_br: Option<&SpExpr>,
    ) -> Self::Output;
    fn fold_list(&mut self, items: &[SpExpr]) -> Self::Output;
    fn fold_cast(&mut self, inner: &SpExpr, ty: &str) -> Self::Output;
    fn fold_block(&mut self, exprs: &[SpExpr]) -> Self::Output;
    fn fold_ghost(&mut self, inner: &SpExpr) -> Self::Output;
    fn fold_apply(&mut self, name: &str, args: &[SpExpr]) -> Self::Output;
    fn fold_let(&mut self, name: &str, value: &SpExpr, body: &SpExpr) -> Self::Output;
    fn fold_match(&mut self, scrutinee: &SpExpr, arms: &[MatchArm]) -> Self::Output;
    fn fold_tuple(&mut self, items: &[SpExpr]) -> Self::Output;
    fn fold_raw(&mut self, tokens: &[String]) -> Self::Output;
}

/// Helpers for String-producing folders to avoid repeating
/// `let v: Vec<String> = xs.iter().map(|e| self.fold_expr(e)).collect(); v.join(...)`
pub fn fold_joined(
    f: &mut (impl ExprFolder<Output = String> + ?Sized),
    items: &[SpExpr],
    sep: &str,
) -> String {
    let parts: Vec<String> = items.iter().map(|e| f.fold_expr(e)).collect();
    parts.join(sep)
}

pub fn fold_arg_list(
    f: &mut (impl ExprFolder<Output = String> + ?Sized),
    args: &[SpExpr],
) -> String {
    fold_joined(f, args, ", ")
}

/// Format a literal as it would appear in Assura/Rust source (used by folders).
pub fn literal_to_string(lit: &Literal) -> String {
    match lit {
        Literal::Int(s) | Literal::Float(s) => s.clone(),
        Literal::Str(s) => format!("\"{s}\""),
        Literal::Bool(b) => b.to_string(),
    }
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
                format!("{}<{}>", name, Self::join_type_list(args, ", "))
            }
            TypeExpr::Tuple(elems) => {
                format!("({})", Self::join_type_list(elems, ", "))
            }
            TypeExpr::Fn { params, ret } => {
                format!(
                    "fn({}) -> {}",
                    Self::join_type_list(params, ", "),
                    ret.to_display_string()
                )
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

    fn join_type_list(items: &[TypeExpr], sep: &str) -> String {
        let s: Vec<String> = items.iter().map(|e| e.to_display_string()).collect();
        s.join(sep)
    }

    /// Convert back to raw token strings (bridge for consumers that need `Vec<String>`).
    pub fn to_tokens(&self) -> Vec<String> {
        match self {
            TypeExpr::Named(name) => {
                // If the name contains spaces (from fallback join in try_parse_type_tokens),
                // split back into individual tokens so downstream consumers like
                // map_type_tokens can process them individually.
                if name.contains(' ') {
                    name.split_whitespace().map(|s| s.to_string()).collect()
                } else {
                    vec![name.clone()]
                }
            }
            TypeExpr::Generic(name, args) => {
                let mut tokens = vec![name.clone(), "<".to_string()];
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        tokens.push(",".to_string());
                    }
                    tokens.extend(arg.to_tokens());
                }
                tokens.push(">".to_string());
                tokens
            }
            TypeExpr::Tuple(elems) => {
                let mut tokens = vec!["(".to_string()];
                for (i, elem) in elems.iter().enumerate() {
                    if i > 0 {
                        tokens.push(",".to_string());
                    }
                    tokens.extend(elem.to_tokens());
                }
                tokens.push(")".to_string());
                tokens
            }
            TypeExpr::Fn { params, ret } => {
                let mut tokens = vec!["fn".to_string(), "(".to_string()];
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        tokens.push(",".to_string());
                    }
                    tokens.extend(p.to_tokens());
                }
                tokens.push(")".to_string());
                tokens.push("->".to_string());
                tokens.extend(ret.to_tokens());
                tokens
            }
            TypeExpr::Refined { base, predicate } => {
                let mut tokens = vec!["{".to_string(), "x".to_string(), ":".to_string()];
                tokens.extend(base.to_tokens());
                tokens.push("|".to_string());
                tokens.push(predicate.clone());
                tokens.push("}".to_string());
                tokens
            }
            TypeExpr::Unit => vec!["(".to_string(), ")".to_string()],
        }
    }
}

impl std::fmt::Display for TypeExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_display_string())
    }
}

/// Best-effort parse of raw type token strings into a structured `TypeExpr`.
///
/// Returns `None` only for empty slices that cannot be interpreted.
pub fn try_parse_type_tokens(tokens: &[String]) -> Option<TypeExpr> {
    if tokens.is_empty() {
        return Some(TypeExpr::Unit);
    }
    if tokens.len() == 1 {
        if tokens[0] == "()" {
            return Some(TypeExpr::Unit);
        }
        return Some(TypeExpr::Named(tokens[0].clone()));
    }
    // "(", ")" as two separate tokens -> Unit
    if tokens.len() == 2 && tokens[0] == "(" && tokens[1] == ")" {
        return Some(TypeExpr::Unit);
    }

    // Strip taint annotations: `T @ taint : label` -> just parse `T`
    // Also handles `T @` (trailing @)
    if let Some(at_pos) = tokens.iter().position(|t| t == "@") {
        return try_parse_type_tokens(&tokens[..at_pos]);
    }

    // Refinement type: `{ v : T | predicate }` or `{ T | predicate }` -> Refined
    if tokens.first().map(|s| s.as_str()) == Some("{") {
        // Try short form first: `{ T | predicate }` (no binder variable / colon)
        // This form is produced by TypeExpr::to_display_string
        if let Some(pipe_pos) = tokens.iter().position(|t| t == "|") {
            let has_colon_before_pipe = tokens[1..pipe_pos].iter().any(|t| t == ":");
            if !has_colon_before_pipe {
                // No colon before the pipe: short form `{ T | ... }`
                let base_tokens = &tokens[1..pipe_pos];
                let base = try_parse_type_tokens(base_tokens)
                    .unwrap_or_else(|| TypeExpr::Named("Unknown".into()));
                let pred_end = tokens.len()
                    - if tokens.last().map(|s| s.as_str()) == Some("}") {
                        1
                    } else {
                        0
                    };
                let pred = tokens[pipe_pos + 1..pred_end].join(" ");
                return Some(TypeExpr::Refined {
                    base: Box::new(base),
                    predicate: pred,
                });
            }
        }
        // Long form: `{ v : T | predicate }` (with binder variable and colon)
        if let Some(colon_pos) = tokens.iter().position(|t| t == ":") {
            // Find the pipe separating type from predicate
            let mut base_end = colon_pos + 1;
            let mut angle = 0i32;
            while base_end < tokens.len() {
                match tokens[base_end].as_str() {
                    "<" => angle += 1,
                    ">" if angle > 0 => angle -= 1,
                    "|" if angle == 0 => break,
                    "}" if angle == 0 => break,
                    _ => {}
                }
                base_end += 1;
            }
            let base_tokens = &tokens[colon_pos + 1..base_end];
            let base = try_parse_type_tokens(base_tokens)
                .unwrap_or_else(|| TypeExpr::Named("Unknown".into()));
            // Collect predicate tokens (between | and })
            let pred = if base_end < tokens.len() && tokens[base_end] == "|" {
                let pred_end = tokens.len()
                    - if tokens.last().map(|s| s.as_str()) == Some("}") {
                        1
                    } else {
                        0
                    };
                tokens[base_end + 1..pred_end].join(" ")
            } else {
                String::new()
            };
            return Some(TypeExpr::Refined {
                base: Box::new(base),
                predicate: pred,
            });
        }
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
// Expression helpers
// ---------------------------------------------------------------------------

/// Check if an expression references the `result` identifier.
///
/// Used by both the type checker (clause quality warnings) and the SMT
/// verifier (unconstrained-result skip logic).  Lives here so every
/// downstream crate shares a single exhaustive implementation.
pub fn expr_references_result(expr: &SpExpr) -> bool {
    match &expr.node {
        Expr::Ident(name) => name == "result",
        Expr::BinOp { lhs, rhs, .. } => expr_references_result(lhs) || expr_references_result(rhs),
        Expr::UnaryOp { expr: e, .. }
        | Expr::Old(e)
        | Expr::Cast { expr: e, .. }
        | Expr::Ghost(e) => expr_references_result(e),
        Expr::Call { func, args } => {
            expr_references_result(func) || args.iter().any(expr_references_result)
        }
        Expr::MethodCall { receiver, args, .. } => {
            expr_references_result(receiver) || args.iter().any(expr_references_result)
        }
        Expr::Field(recv, _) => expr_references_result(recv),
        Expr::Index { expr: e, index } => {
            expr_references_result(e) || expr_references_result(index)
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_references_result(cond)
                || expr_references_result(then_branch)
                || else_branch
                    .as_ref()
                    .is_some_and(|e| expr_references_result(e))
        }
        Expr::Forall { body, .. } | Expr::Exists { body, .. } => expr_references_result(body),
        Expr::Let { value, body, .. } => {
            expr_references_result(value) || expr_references_result(body)
        }
        Expr::Match { scrutinee, arms } => {
            expr_references_result(scrutinee)
                || arms.iter().any(|a| expr_references_result(&a.body))
        }
        Expr::Tuple(items) | Expr::List(items) | Expr::Block(items) => {
            items.iter().any(expr_references_result)
        }
        Expr::Raw(tokens) => tokens.iter().any(|t| t == "result"),
        Expr::Literal(_) | Expr::Apply { .. } => false,
    }
}

// ---------------------------------------------------------------------------
// Shared clause parameter extraction
// ---------------------------------------------------------------------------

/// A parsed parameter from an input/output clause.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedParam {
    pub name: String,
    /// Structured type expression. `None` if untyped.
    pub ty: Option<TypeExpr>,
}

/// Extract `(name, type)` parameter pairs from a clause body expression.
///
/// Handles all known patterns:
/// - `input(a: Int, b: Bool)` -> Call with Cast args
/// - `input(x)` -> Call with Ident args (untyped)
/// - `input { a: Int }` -> Block with Cast elements
/// - Raw token fallback: `["a", ":", "Int", ",", "b", ":", "Bool"]`
pub fn extract_clause_params(body: &SpExpr) -> Vec<ParsedParam> {
    let mut params = Vec::new();
    extract_clause_params_inner(body, &mut params);
    params
}

fn extract_clause_params_inner(body: &SpExpr, params: &mut Vec<ParsedParam>) {
    match &body.node {
        Expr::Call { args, .. } => {
            for arg in args {
                extract_single_param(arg, params);
            }
        }
        Expr::Cast { expr: inner, ty } => {
            if let Expr::Ident(name) = &inner.node {
                let ty_tokens = vec![ty.clone()];
                let parsed = try_parse_type_tokens(&ty_tokens);
                params.push(ParsedParam {
                    name: name.clone(),
                    ty: parsed,
                });
            }
        }
        Expr::Ident(name) => {
            params.push(ParsedParam {
                name: name.clone(),
                ty: None,
            });
        }
        Expr::Tuple(items) | Expr::Block(items) => {
            for item in items {
                extract_single_param(item, params);
            }
        }
        Expr::Raw(tokens) => extract_clause_params_from_raw(tokens, params),
        _ => {}
    }
}

fn extract_single_param(expr: &SpExpr, params: &mut Vec<ParsedParam>) {
    match &expr.node {
        Expr::Cast { expr: inner, ty } => {
            if let Expr::Ident(name) = &inner.node {
                let ty_tokens = vec![ty.clone()];
                let parsed = try_parse_type_tokens(&ty_tokens);
                params.push(ParsedParam {
                    name: name.clone(),
                    ty: parsed,
                });
            }
        }
        Expr::Ident(name) => {
            params.push(ParsedParam {
                name: name.clone(),
                ty: None,
            });
        }
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
            let ty_tokens: Vec<String> = tokens[type_start..j].to_vec();
            let parsed = try_parse_type_tokens(&ty_tokens);
            params.push(ParsedParam { name, ty: parsed });
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
                params.push(ParsedParam { name, ty: None });
            }
            i += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Type / Enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct TypeDef {
    pub name: String,
    pub type_params: Vec<String>,
    pub body: TypeBody,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeBody {
    Alias(Vec<String>),
    Struct(Vec<FieldDef>),
    Refined(Vec<String>),
    Empty,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldDef {
    pub name: String,
    /// Structured type expression. `None` if untyped.
    pub ty: Option<TypeExpr>,
    pub is_pub: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumDef {
    pub name: String,
    pub type_params: Vec<String>,
    pub variants: Vec<EnumVariant>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariant {
    pub name: String,
    pub fields: Vec<String>,
}

// ---------------------------------------------------------------------------
// Extern / Fn
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct ExternDecl {
    pub name: String,
    pub params: Vec<Param>,
    /// Structured return type expression. `None` if no return type.
    pub return_ty: Option<TypeExpr>,
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
#[derive(Debug, Clone, PartialEq)]
pub struct BindDecl {
    /// The contract name (the `as Ident` part).
    pub name: String,
    /// The Rust function path being bound (the string literal).
    pub target_path: String,
    pub params: Vec<Param>,
    /// Structured return type expression. `None` if no return type.
    pub return_ty: Option<TypeExpr>,
    pub clauses: Vec<Clause>,
}

/// Ghost prophecy variable: `ghost prophecy <name>: <type>`
#[derive(Debug, Clone, PartialEq)]
pub struct ProphecyDecl {
    pub name: String,
    /// Structured type expression. `None` if untyped.
    pub ty: Option<TypeExpr>,
}

// ---------------------------------------------------------------------------
// Codec Registry (FMT.4)
// ---------------------------------------------------------------------------

/// A codec registry declaration: `codec_registry <name> { output: <type>, codec ... }`
#[derive(Debug, Clone, PartialEq)]
pub struct CodecRegistryDecl {
    pub name: String,
    /// The common output type (e.g., `ImageOutput`)
    pub output_type: Vec<String>,
    /// Individual codec entries
    pub codecs: Vec<CodecEntry>,
}

/// A single codec in a registry: `codec <name> { magic: [...], decoder: <fn>, contracts: { ... } }`
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone, PartialEq)]
pub enum MagicPattern {
    /// Exact or prefix byte pattern: `[0x89, 0x50, ...]` or `[0xFF, 0xD8, ..]`
    Bytes { bytes: Vec<u8>, prefix: bool },
    /// File extension matching: `extension("png", "apng")`
    Extension(Vec<String>),
    /// Probe function: `probe(is_tga)`
    Probe(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FnDef {
    pub name: String,
    pub is_ghost: bool,
    pub is_lemma: bool,
    pub params: Vec<Param>,
    /// Structured return type expression. `None` if no return type.
    pub return_ty: Option<TypeExpr>,
    pub clauses: Vec<Clause>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    /// Structured type expression. `None` if untyped.
    pub ty: Option<TypeExpr>,
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct ServiceDecl {
    pub name: String,
    pub items: Vec<ServiceItem>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ServiceItem {
    TypeDef(TypeDef),
    EnumDef(EnumDef),
    States(Vec<String>),
    Operation { name: String, clauses: Vec<Clause> },
    Query { name: String, clauses: Vec<Clause> },
    Invariant(SpExpr),
    Other { kind: String, body: SpExpr },
}

/// Convert an `Expr` to a human-readable string representation.
mod display;
pub use display::{expr_to_string, negate_comparison, negate_expr, truncate};

#[cfg(test)]
#[path = "ast_tests.rs"]
mod tests;
