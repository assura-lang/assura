//! High-level Intermediate Representation (HIR) for the Assura compiler.
//!
//! The HIR sits between the parsed AST (assura-parser) and the type checker
//! (assura-types). It desugars syntactic sugar, resolves names to unique IDs,
//! replaces raw token sequences with structured types/expressions, and
//! normalizes clause representations.
//!
//! Pipeline: Source -> AST (parser) -> ResolvedFile (resolve) -> HirFile (hir) -> TypedFile (types)

use assura_parser::ast::{self, Span};
use assura_resolve::{ResolvedFile, SymbolTable};

mod lower;

pub use lower::lower;

// ---------------------------------------------------------------------------
// Unique identifiers
// ---------------------------------------------------------------------------

/// A unique identifier for a definition in the HIR. Maps to a symbol table
/// index when the name was resolved, or contains just the name if unresolved.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DefId {
    /// Resolved to a symbol table index.
    Resolved(usize),
    /// Unresolved name (external import, built-in, or forward reference).
    Unresolved(String),
}

impl DefId {
    /// Returns the human-readable name for this definition.
    pub fn name<'a>(&'a self, symbols: &'a SymbolTable) -> &'a str {
        match self {
            DefId::Resolved(idx) => &symbols.symbols[*idx].name,
            DefId::Unresolved(name) => name,
        }
    }
}

// ---------------------------------------------------------------------------
// Top-level HIR file
// ---------------------------------------------------------------------------

/// The top-level HIR representation of a source file, produced by lowering
/// a `ResolvedFile`.
#[derive(Debug, Clone)]
pub struct HirFile {
    /// The original resolved file (preserved for backward compatibility
    /// with the type checker during migration).
    pub resolved: ResolvedFile,
    /// All declarations in the file, in source order.
    pub decls: Vec<HirDecl>,
}

// ---------------------------------------------------------------------------
// Declarations
// ---------------------------------------------------------------------------

/// A top-level declaration in the HIR.
#[derive(Debug, Clone)]
pub struct HirDecl {
    pub kind: HirDeclKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum HirDeclKind {
    Contract(HirContract),
    Service(HirService),
    TypeDef(HirTypeDef),
    EnumDef(HirEnumDef),
    Extern(HirExtern),
    FnDef(HirFnDef),
    Block(HirBlock),
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct HirContract {
    pub id: DefId,
    pub name: String,
    pub type_params: Vec<String>,
    pub clauses: Vec<HirClause>,
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct HirService {
    pub id: DefId,
    pub name: String,
    pub items: Vec<HirServiceItem>,
}

#[derive(Debug, Clone)]
pub enum HirServiceItem {
    TypeDef(HirTypeDef),
    EnumDef(HirEnumDef),
    States(Vec<String>),
    Operation {
        name: String,
        clauses: Vec<HirClause>,
    },
    Query {
        name: String,
        clauses: Vec<HirClause>,
    },
    Invariant(HirExpr),
    Other {
        kind: String,
        body: HirExpr,
    },
}

// ---------------------------------------------------------------------------
// Type definition
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct HirTypeDef {
    pub id: DefId,
    pub name: String,
    pub type_params: Vec<String>,
    pub body: HirTypeBody,
}

#[derive(Debug, Clone)]
pub enum HirTypeBody {
    /// Type alias: `type Foo = Bar`
    Alias(HirType),
    /// Struct: `type Foo { field: Type }`
    Struct(Vec<HirFieldDef>),
    /// Refined type: `type Foo = { x: Int | x > 0 }`
    Refined {
        base: HirType,
        predicate: String,
    },
    Empty,
}

#[derive(Debug, Clone)]
pub struct HirFieldDef {
    pub name: String,
    pub ty: HirType,
    pub is_pub: bool,
}

// ---------------------------------------------------------------------------
// Enum definition
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct HirEnumDef {
    pub id: DefId,
    pub name: String,
    pub type_params: Vec<String>,
    pub variants: Vec<HirEnumVariant>,
}

#[derive(Debug, Clone)]
pub struct HirEnumVariant {
    pub name: String,
    pub fields: Vec<HirType>,
}

// ---------------------------------------------------------------------------
// Extern declaration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct HirExtern {
    pub id: DefId,
    pub name: String,
    pub params: Vec<HirParam>,
    pub return_ty: HirType,
    pub clauses: Vec<HirClause>,
}

// ---------------------------------------------------------------------------
// Function definition
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct HirFnDef {
    pub id: DefId,
    pub name: String,
    pub is_ghost: bool,
    pub is_lemma: bool,
    pub params: Vec<HirParam>,
    pub return_ty: HirType,
    pub clauses: Vec<HirClause>,
}

// ---------------------------------------------------------------------------
// Parameters
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct HirParam {
    pub name: String,
    pub ty: HirType,
}

// ---------------------------------------------------------------------------
// Block declarations (feature, table, spec, etc.)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct HirBlock {
    pub kind: String,
    pub name: String,
    pub value: Option<HirExpr>,
    pub clauses: Vec<HirClause>,
}

// ---------------------------------------------------------------------------
// Types (structured, not raw tokens)
// ---------------------------------------------------------------------------

/// A structured type representation, replacing raw `Vec<String>` tokens.
#[derive(Debug, Clone, PartialEq)]
pub enum HirType {
    /// Named type: `Int`, `Bool`, `MyStruct`, etc.
    Named(String),
    /// Generic application: `List<Int>`, `Map<String, Int>`
    Generic(String, Vec<HirType>),
    /// Tuple type: `(Int, Bool)`
    Tuple(Vec<HirType>),
    /// Function type: `(Int, Int) -> Bool`
    Fn {
        params: Vec<HirType>,
        ret: Box<HirType>,
    },
    /// Refined type: `{ x: Int | x > 0 }`
    Refined {
        base: Box<HirType>,
        predicate: String,
    },
    /// Unit type (empty return / void)
    Unit,
    /// Unresolved type (raw tokens that could not be parsed)
    Unresolved(Vec<String>),
}

impl std::fmt::Display for HirType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HirType::Named(n) => write!(f, "{n}"),
            HirType::Generic(n, args) => {
                write!(f, "{n}<")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{a}")?;
                }
                write!(f, ">")
            }
            HirType::Tuple(elems) => {
                write!(f, "(")?;
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{e}")?;
                }
                write!(f, ")")
            }
            HirType::Fn { params, ret } => {
                write!(f, "(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{p}")?;
                }
                write!(f, ") -> {ret}")
            }
            HirType::Refined { base, predicate } => {
                write!(f, "{{ x: {base} | {predicate} }}")
            }
            HirType::Unit => write!(f, "Unit"),
            HirType::Unresolved(tokens) => write!(f, "{}", tokens.join(" ")),
        }
    }
}

// ---------------------------------------------------------------------------
// Clauses
// ---------------------------------------------------------------------------

/// A contract/function clause in the HIR, with structured body.
#[derive(Debug, Clone)]
pub struct HirClause {
    pub kind: HirClauseKind,
    pub body: HirExpr,
}

/// Clause kinds, mirroring `ast::ClauseKind` but without catch-all.
#[derive(Debug, Clone, PartialEq)]
pub enum HirClauseKind {
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
    Other(String),
}

impl From<&ast::ClauseKind> for HirClauseKind {
    fn from(kind: &ast::ClauseKind) -> Self {
        match kind {
            ast::ClauseKind::Requires => HirClauseKind::Requires,
            ast::ClauseKind::Ensures => HirClauseKind::Ensures,
            ast::ClauseKind::Effects => HirClauseKind::Effects,
            ast::ClauseKind::Invariant => HirClauseKind::Invariant,
            ast::ClauseKind::Modifies => HirClauseKind::Modifies,
            ast::ClauseKind::Input => HirClauseKind::Input,
            ast::ClauseKind::Output => HirClauseKind::Output,
            ast::ClauseKind::Errors => HirClauseKind::Errors,
            ast::ClauseKind::Rule => HirClauseKind::Rule,
            ast::ClauseKind::DataFlow => HirClauseKind::DataFlow,
            ast::ClauseKind::MustNot => HirClauseKind::MustNot,
            ast::ClauseKind::Decreases => HirClauseKind::Decreases,
            ast::ClauseKind::Other(s) => HirClauseKind::Other(s.clone()),
        }
    }
}

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

/// HIR expressions. Same structure as AST `Expr` but:
/// - No `Raw(Vec<String>)` variant (all expressions are structured)
/// - Identifiers carry optional `DefId` for resolved names
#[derive(Debug, Clone)]
pub enum HirExpr {
    /// Integer, float, string, or boolean literal
    Literal(ast::Literal),
    /// Named reference with optional resolution
    Ident { name: String, def_id: Option<DefId> },
    /// Field access: `expr.field`
    Field(Box<HirExpr>, String),
    /// Method call: `expr.method(args)`
    MethodCall {
        receiver: Box<HirExpr>,
        method: String,
        args: Vec<HirExpr>,
    },
    /// Function call: `f(args)`
    Call {
        func: Box<HirExpr>,
        args: Vec<HirExpr>,
    },
    /// Index access: `expr[index]`
    Index {
        expr: Box<HirExpr>,
        index: Box<HirExpr>,
    },
    /// Binary operation
    BinOp {
        lhs: Box<HirExpr>,
        op: ast::BinOp,
        rhs: Box<HirExpr>,
    },
    /// Unary operation
    UnaryOp {
        op: ast::UnaryOp,
        expr: Box<HirExpr>,
    },
    /// `old(expr)` for postconditions
    Old(Box<HirExpr>),
    /// `forall var in domain: body`
    Forall {
        var: String,
        domain: Box<HirExpr>,
        body: Box<HirExpr>,
    },
    /// `exists var in domain: body`
    Exists {
        var: String,
        domain: Box<HirExpr>,
        body: Box<HirExpr>,
    },
    /// `if cond then expr [else expr]`
    If {
        cond: Box<HirExpr>,
        then_branch: Box<HirExpr>,
        else_branch: Option<Box<HirExpr>>,
    },
    /// Parenthesized expression
    Paren(Box<HirExpr>),
    /// List literal: `[a, b, c]`
    List(Vec<HirExpr>),
    /// Type cast: `expr as Type`
    Cast { expr: Box<HirExpr>, ty: String },
    /// Sequence of expressions (e.g., parameter lists)
    Block(Vec<HirExpr>),
    /// Ghost block: verified but erased at runtime
    Ghost(Box<HirExpr>),
    /// Apply a lemma
    Apply {
        lemma_name: String,
        args: Vec<HirExpr>,
    },
    /// Let binding
    Let {
        name: String,
        value: Box<HirExpr>,
        body: Box<HirExpr>,
    },
    /// Match expression
    Match {
        scrutinee: Box<HirExpr>,
        arms: Vec<HirMatchArm>,
    },
    /// Tuple expression
    Tuple(Vec<HirExpr>),
    /// Raw tokens preserved for non-expression clauses (effects, modifies,
    /// input, output). These are not "unparsed fallback" but intentionally
    /// raw because they contain token lists, not expressions.
    RawTokens(Vec<String>),
}

#[derive(Debug, Clone)]
pub struct HirMatchArm {
    pub pattern: ast::Pattern,
    pub body: HirExpr,
}

// ---------------------------------------------------------------------------
// Conversion utilities: HirExpr -> ast::Expr
// ---------------------------------------------------------------------------

impl HirExpr {
    /// Convert back to an AST expression for backward compatibility with
    /// the type checker during migration.
    pub fn to_ast_expr(&self) -> ast::Expr {
        match self {
            HirExpr::Literal(lit) => ast::Expr::Literal(lit.clone()),
            HirExpr::Ident { name, .. } => ast::Expr::Ident(name.clone()),
            HirExpr::Field(base, field) => {
                ast::Expr::Field(Box::new(base.to_ast_expr()), field.clone())
            }
            HirExpr::MethodCall {
                receiver,
                method,
                args,
            } => ast::Expr::MethodCall {
                receiver: Box::new(receiver.to_ast_expr()),
                method: method.clone(),
                args: args.iter().map(|a| a.to_ast_expr()).collect(),
            },
            HirExpr::Call { func, args } => ast::Expr::Call {
                func: Box::new(func.to_ast_expr()),
                args: args.iter().map(|a| a.to_ast_expr()).collect(),
            },
            HirExpr::Index { expr, index } => ast::Expr::Index {
                expr: Box::new(expr.to_ast_expr()),
                index: Box::new(index.to_ast_expr()),
            },
            HirExpr::BinOp { lhs, op, rhs } => ast::Expr::BinOp {
                lhs: Box::new(lhs.to_ast_expr()),
                op: op.clone(),
                rhs: Box::new(rhs.to_ast_expr()),
            },
            HirExpr::UnaryOp { op, expr } => ast::Expr::UnaryOp {
                op: op.clone(),
                expr: Box::new(expr.to_ast_expr()),
            },
            HirExpr::Old(e) => ast::Expr::Old(Box::new(e.to_ast_expr())),
            HirExpr::Forall { var, domain, body } => ast::Expr::Forall {
                var: var.clone(),
                domain: Box::new(domain.to_ast_expr()),
                body: Box::new(body.to_ast_expr()),
            },
            HirExpr::Exists { var, domain, body } => ast::Expr::Exists {
                var: var.clone(),
                domain: Box::new(domain.to_ast_expr()),
                body: Box::new(body.to_ast_expr()),
            },
            HirExpr::If {
                cond,
                then_branch,
                else_branch,
            } => ast::Expr::If {
                cond: Box::new(cond.to_ast_expr()),
                then_branch: Box::new(then_branch.to_ast_expr()),
                else_branch: else_branch.as_ref().map(|e| Box::new(e.to_ast_expr())),
            },
            HirExpr::Paren(e) => ast::Expr::Paren(Box::new(e.to_ast_expr())),
            HirExpr::List(items) => {
                ast::Expr::List(items.iter().map(|i| i.to_ast_expr()).collect())
            }
            HirExpr::Cast { expr, ty } => ast::Expr::Cast {
                expr: Box::new(expr.to_ast_expr()),
                ty: ty.clone(),
            },
            HirExpr::Block(items) => {
                ast::Expr::Block(items.iter().map(|i| i.to_ast_expr()).collect())
            }
            HirExpr::Ghost(e) => ast::Expr::Ghost(Box::new(e.to_ast_expr())),
            HirExpr::Apply { lemma_name, args } => ast::Expr::Apply {
                lemma_name: lemma_name.clone(),
                args: args.iter().map(|a| a.to_ast_expr()).collect(),
            },
            HirExpr::Let { name, value, body } => ast::Expr::Let {
                name: name.clone(),
                value: Box::new(value.to_ast_expr()),
                body: Box::new(body.to_ast_expr()),
            },
            HirExpr::Match { scrutinee, arms } => ast::Expr::Match {
                scrutinee: Box::new(scrutinee.to_ast_expr()),
                arms: arms
                    .iter()
                    .map(|a| ast::MatchArm {
                        pattern: a.pattern.clone(),
                        body: a.body.to_ast_expr(),
                    })
                    .collect(),
            },
            HirExpr::Tuple(items) => {
                ast::Expr::Tuple(items.iter().map(|i| i.to_ast_expr()).collect())
            }
            HirExpr::RawTokens(tokens) => ast::Expr::Raw(tokens.clone()),
        }
    }
}

// ---------------------------------------------------------------------------
// Conversion utilities: HirClause -> ast::Clause
// ---------------------------------------------------------------------------

impl HirClause {
    /// Convert back to an AST clause for backward compatibility.
    pub fn to_ast_clause(&self) -> ast::Clause {
        ast::Clause {
            kind: self.kind.to_ast_kind(),
            body: self.body.to_ast_expr(),
        }
    }
}

impl HirClauseKind {
    pub fn to_ast_kind(&self) -> ast::ClauseKind {
        match self {
            HirClauseKind::Requires => ast::ClauseKind::Requires,
            HirClauseKind::Ensures => ast::ClauseKind::Ensures,
            HirClauseKind::Effects => ast::ClauseKind::Effects,
            HirClauseKind::Invariant => ast::ClauseKind::Invariant,
            HirClauseKind::Modifies => ast::ClauseKind::Modifies,
            HirClauseKind::Input => ast::ClauseKind::Input,
            HirClauseKind::Output => ast::ClauseKind::Output,
            HirClauseKind::Errors => ast::ClauseKind::Errors,
            HirClauseKind::Rule => ast::ClauseKind::Rule,
            HirClauseKind::DataFlow => ast::ClauseKind::DataFlow,
            HirClauseKind::MustNot => ast::ClauseKind::MustNot,
            HirClauseKind::Decreases => ast::ClauseKind::Decreases,
            HirClauseKind::Other(s) => ast::ClauseKind::Other(s.clone()),
        }
    }
}

// ---------------------------------------------------------------------------
// HirFile -> SourceFile conversion (backward compatibility)
// ---------------------------------------------------------------------------

impl HirFile {
    /// Convert the HIR file back to a `SourceFile` for the type checker.
    /// This is a compatibility layer during migration; eventually the type
    /// checker will operate directly on `HirFile`.
    pub fn to_source_file(&self) -> ast::SourceFile {
        self.resolved.source.clone()
    }

    /// Get the resolved file reference.
    pub fn resolved(&self) -> &ResolvedFile {
        &self.resolved
    }
}

// ---------------------------------------------------------------------------
// Type parsing: Vec<String> -> HirType
// ---------------------------------------------------------------------------

/// Parse raw type tokens into a structured `HirType`.
pub fn parse_type_tokens(tokens: &[String]) -> HirType {
    if tokens.is_empty() {
        return HirType::Unit;
    }

    // Single token: named type
    if tokens.len() == 1 {
        return HirType::Named(tokens[0].clone());
    }

    // Tuple type: (A, B, C)
    if tokens.first().is_some_and(|t| t == "(") && tokens.last().is_some_and(|t| t == ")") {
        let inner = &tokens[1..tokens.len() - 1];
        if inner.is_empty() {
            return HirType::Unit;
        }
        let parts = split_at_commas(inner);
        if parts.len() > 1 {
            return HirType::Tuple(parts.iter().map(|p| parse_type_tokens(p)).collect());
        }
    }

    // Refined type: { x: Base | predicate }
    if tokens.first().is_some_and(|t| t == "{") && tokens.last().is_some_and(|t| t == "}") {
        let inner = &tokens[1..tokens.len() - 1];
        if let Some(pipe_pos) = inner.iter().position(|t| t == "|") {
            // Everything before | is the binder (e.g., "x : Int")
            let binder = &inner[..pipe_pos];
            let predicate = &inner[pipe_pos + 1..];
            // Try to extract the base type from "x : Type"
            if let Some(colon_pos) = binder.iter().position(|t| t == ":") {
                let base_tokens = &binder[colon_pos + 1..];
                let base = parse_type_tokens(base_tokens);
                let pred_str = predicate
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                return HirType::Refined {
                    base: Box::new(base),
                    predicate: pred_str,
                };
            }
        }
    }

    // Generic type: Name<A, B>
    if tokens.len() >= 3 && tokens[1] == "<" && tokens.last().is_some_and(|t| t == ">") {
        let name = tokens[0].clone();
        let inner = &tokens[2..tokens.len() - 1];
        let parts = split_at_commas(inner);
        let args: Vec<HirType> = parts.iter().map(|p| parse_type_tokens(p)).collect();
        return HirType::Generic(name, args);
    }

    // Function type: tokens containing ->
    if let Some(arrow_pos) = tokens.iter().position(|t| t == "->") {
        let param_tokens = &tokens[..arrow_pos];
        let ret_tokens = &tokens[arrow_pos + 1..];
        let ret = parse_type_tokens(ret_tokens);
        // If param_tokens is (A, B), parse as tuple
        if param_tokens.first().is_some_and(|t| t == "(")
            && param_tokens.last().is_some_and(|t| t == ")")
        {
            let inner = &param_tokens[1..param_tokens.len() - 1];
            let parts = split_at_commas(inner);
            let params: Vec<HirType> = parts.iter().map(|p| parse_type_tokens(p)).collect();
            return HirType::Fn {
                params,
                ret: Box::new(ret),
            };
        }
        let params = vec![parse_type_tokens(param_tokens)];
        return HirType::Fn {
            params,
            ret: Box::new(ret),
        };
    }

    // Fallback: preserve as unresolved
    HirType::Unresolved(tokens.to_vec())
}

/// Split token slice at top-level commas (respecting <>, (), {} nesting).
fn split_at_commas(tokens: &[String]) -> Vec<&[String]> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut angle = 0i32;
    let mut paren = 0i32;
    let mut brace = 0i32;

    for (i, tok) in tokens.iter().enumerate() {
        match tok.as_str() {
            "<" => angle += 1,
            ">" if angle > 0 => angle -= 1,
            "(" => paren += 1,
            ")" if paren > 0 => paren -= 1,
            "{" => brace += 1,
            "}" if brace > 0 => brace -= 1,
            "," if angle == 0 && paren == 0 && brace == 0 => {
                parts.push(&tokens[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < tokens.len() {
        parts.push(&tokens[start..]);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- parse_type_tokens tests ----

    #[test]
    fn parse_named_type() {
        assert_eq!(
            parse_type_tokens(&["Int".into()]),
            HirType::Named("Int".into())
        );
    }

    #[test]
    fn parse_unit_type() {
        assert_eq!(parse_type_tokens(&[]), HirType::Unit);
    }

    #[test]
    fn parse_generic_type() {
        let tokens: Vec<String> = vec!["List", "<", "Int", ">"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(
            parse_type_tokens(&tokens),
            HirType::Generic("List".into(), vec![HirType::Named("Int".into())])
        );
    }

    #[test]
    fn parse_multi_generic() {
        let tokens: Vec<String> = vec!["Map", "<", "String", ",", "Int", ">"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(
            parse_type_tokens(&tokens),
            HirType::Generic(
                "Map".into(),
                vec![
                    HirType::Named("String".into()),
                    HirType::Named("Int".into())
                ]
            )
        );
    }

    #[test]
    fn parse_refined_type() {
        let tokens: Vec<String> = vec!["{", "x", ":", "Int", "|", "x", ">", "0", "}"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(
            parse_type_tokens(&tokens),
            HirType::Refined {
                base: Box::new(HirType::Named("Int".into())),
                predicate: "x > 0".into(),
            }
        );
    }

    #[test]
    fn parse_tuple_type() {
        let tokens: Vec<String> = vec!["(", "Int", ",", "Bool", ")"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(
            parse_type_tokens(&tokens),
            HirType::Tuple(vec![
                HirType::Named("Int".into()),
                HirType::Named("Bool".into()),
            ])
        );
    }

    #[test]
    fn hir_type_display() {
        assert_eq!(HirType::Named("Int".into()).to_string(), "Int");
        assert_eq!(
            HirType::Generic("List".into(), vec![HirType::Named("Int".into())]).to_string(),
            "List<Int>"
        );
        assert_eq!(HirType::Unit.to_string(), "Unit");
    }

    // ---- HirExpr round-trip tests ----

    #[test]
    fn hir_expr_to_ast_literal() {
        let hir = HirExpr::Literal(ast::Literal::Int("42".into()));
        let ast_expr = hir.to_ast_expr();
        assert!(matches!(ast_expr, ast::Expr::Literal(ast::Literal::Int(s)) if s == "42"));
    }

    #[test]
    fn hir_expr_to_ast_ident() {
        let hir = HirExpr::Ident {
            name: "x".into(),
            def_id: Some(DefId::Resolved(0)),
        };
        let ast_expr = hir.to_ast_expr();
        assert!(matches!(ast_expr, ast::Expr::Ident(s) if s == "x"));
    }

    #[test]
    fn hir_expr_to_ast_binop() {
        let hir = HirExpr::BinOp {
            lhs: Box::new(HirExpr::Ident {
                name: "a".into(),
                def_id: None,
            }),
            op: ast::BinOp::Add,
            rhs: Box::new(HirExpr::Literal(ast::Literal::Int("1".into()))),
        };
        let ast_expr = hir.to_ast_expr();
        assert!(matches!(
            ast_expr,
            ast::Expr::BinOp {
                op: ast::BinOp::Add,
                ..
            }
        ));
    }

    #[test]
    fn hir_clause_to_ast_clause() {
        let clause = HirClause {
            kind: HirClauseKind::Requires,
            body: HirExpr::Literal(ast::Literal::Bool(true)),
        };
        let ast_clause = clause.to_ast_clause();
        assert_eq!(ast_clause.kind, ast::ClauseKind::Requires);
        assert!(matches!(
            ast_clause.body,
            ast::Expr::Literal(ast::Literal::Bool(true))
        ));
    }

    #[test]
    fn hir_raw_tokens_roundtrip() {
        let tokens = vec!["io".into(), ".".into(), "read".into()];
        let hir = HirExpr::RawTokens(tokens.clone());
        let ast_expr = hir.to_ast_expr();
        assert!(matches!(ast_expr, ast::Expr::Raw(t) if t == tokens));
    }

    // ---- DefId tests ----

    #[test]
    fn def_id_unresolved_name() {
        let table = SymbolTable {
            symbols: vec![],
            scopes: vec![],
        };
        let id = DefId::Unresolved("Foo".into());
        assert_eq!(id.name(&table), "Foo");
    }
}
