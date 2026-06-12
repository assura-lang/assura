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
    Block { kind: String, name: String, body: Vec<Clause> },
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
    pub tokens: Vec<String>,
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
    Invariant(Vec<String>),
    Other { kind: String, body: Vec<String> },
}
