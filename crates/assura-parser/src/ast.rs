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
        /// Optional inline value (e.g., `feature_max X: Nat = 280` stores `["280"]`).
        value: Option<Vec<String>>,
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
    Decreases,
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
// Shared clause parameter extraction
// ---------------------------------------------------------------------------

/// A parsed parameter from an input/output clause.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedParam {
    pub name: String,
    /// Raw type tokens (e.g., `["List", "<", "Int", ">"]`). Empty if untyped.
    pub ty: Vec<String>,
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
                params.push(ParsedParam {
                    name: name.clone(),
                    ty: vec![ty.clone()],
                });
            }
        }
        Expr::Ident(name) => {
            params.push(ParsedParam {
                name: name.clone(),
                ty: vec![],
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
                params.push(ParsedParam {
                    name: name.clone(),
                    ty: vec![ty.clone()],
                });
            }
        }
        Expr::Ident(name) => {
            params.push(ParsedParam {
                name: name.clone(),
                ty: vec![],
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
            params.push(ParsedParam { name, ty });
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
                params.push(ParsedParam { name, ty: vec![] });
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
