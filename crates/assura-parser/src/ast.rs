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
    FnDef(FnDef),
    /// Catch-all for extended syntax (feature, incremental, liveness, etc.)
    Block {
        kind: String,
        name: String,
        body: Vec<Clause>,
    },
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ContractDecl {
    pub name: String,
    pub type_params: Vec<String>,
    pub clauses: Vec<Clause>,
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
    Other(String),
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
    /// Unparsed token sequence (fallback)
    Raw(Vec<String>),
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
    pub clauses: Vec<Clause>,
}

#[derive(Debug, Clone)]
pub struct FnDef {
    pub name: String,
    pub is_ghost: bool,
    pub is_lemma: bool,
    pub params: Vec<Param>,
    pub return_ty: Vec<String>,
    pub clauses: Vec<Clause>,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: Vec<String>,
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
