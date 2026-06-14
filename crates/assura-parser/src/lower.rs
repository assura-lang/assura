//! CST -> AST lowering: convert a rowan `SyntaxNode` tree into the
//! existing `ast::*` types consumed by all downstream crates.
//!
//! This module walks the concrete syntax tree produced by the grammar
//! module and constructs the abstract syntax tree types defined in
//! `ast.rs`. The lowering is intentionally tolerant of missing nodes
//! (returns empty strings / empty vecs / defaults) so that it works
//! with error-recovered partial trees.

use crate::ast::*;
use crate::syntax_kind::SyntaxKind;

type SyntaxNode = crate::syntax_kind::SyntaxNode;

/// Lower a `SOURCE_FILE` node into an `ast::SourceFile`.
pub fn lower_source_file(root: &SyntaxNode) -> SourceFile {
    let mut project = None;
    let mut module = None;
    let mut imports = Vec::new();
    let mut decls = Vec::new();

    for child in root.children() {
        match child.kind() {
            SyntaxKind::PROJECT_DECL => project = Some(lower_project(&child)),
            SyntaxKind::MODULE_DECL => module = Some(lower_module(&child)),
            SyntaxKind::IMPORT_DECL => imports.push(lower_import(&child)),
            SyntaxKind::CONTRACT_DECL => {
                let span = span_of(&child);
                decls.push(Spanned {
                    node: Decl::Contract(lower_contract(&child)),
                    span,
                });
            }
            SyntaxKind::SERVICE_DECL => {
                let span = span_of(&child);
                decls.push(Spanned {
                    node: Decl::Service(lower_service(&child)),
                    span,
                });
            }
            SyntaxKind::TYPE_DEF => {
                let span = span_of(&child);
                decls.push(Spanned {
                    node: Decl::TypeDef(lower_type_def(&child)),
                    span,
                });
            }
            SyntaxKind::ENUM_DEF => {
                let span = span_of(&child);
                decls.push(Spanned {
                    node: Decl::EnumDef(lower_enum_def(&child)),
                    span,
                });
            }
            SyntaxKind::EXTERN_DECL => {
                let span = span_of(&child);
                decls.push(Spanned {
                    node: Decl::Extern(lower_extern(&child)),
                    span,
                });
            }
            SyntaxKind::BIND_DECL => {
                let span = span_of(&child);
                decls.push(Spanned {
                    node: Decl::Bind(lower_bind(&child)),
                    span,
                });
            }
            SyntaxKind::FN_DEF => {
                let span = span_of(&child);
                decls.push(Spanned {
                    node: Decl::FnDef(lower_fn_def(&child)),
                    span,
                });
            }
            SyntaxKind::GENERIC_BLOCK => {
                let span = span_of(&child);
                decls.push(Spanned {
                    node: lower_generic_block(&child),
                    span,
                });
            }
            _ => {}
        }
    }

    SourceFile {
        project,
        module,
        imports,
        decls,
    }
}

// -----------------------------------------------------------------
// Utilities
// -----------------------------------------------------------------

/// Get the byte-offset span of a node.
fn span_of(n: &SyntaxNode) -> Span {
    let range = n.text_range();
    (range.start().into())..(range.end().into())
}

/// Collect all token text from a node, optionally filtering by kind.
fn collect_text(n: &SyntaxNode) -> String {
    n.text().to_string()
}

/// Collect token texts as a Vec<String>, one per token.
fn collect_token_texts(n: &SyntaxNode) -> Vec<String> {
    n.descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| {
            let k = t.kind();
            k != SyntaxKind::WHITESPACE && k != SyntaxKind::COMMENT
        })
        .map(|t| t.text().to_string())
        .collect()
}

/// Find the first IDENT token in a node and return its text.
fn first_ident(n: &SyntaxNode) -> String {
    n.children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| t.kind() == SyntaxKind::IDENT)
        .map(|t| t.text().to_string())
        .unwrap_or_default()
}

/// Find a child node of specific kind.
fn find_child(n: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxNode> {
    n.children().find(|c| c.kind() == kind)
}

/// Check if a node contains a specific token kind.
fn has_token(n: &SyntaxNode, kind: SyntaxKind) -> bool {
    n.children_with_tokens().any(|el| el.kind() == kind)
}

// -----------------------------------------------------------------
// Project / Module / Import
// -----------------------------------------------------------------

fn lower_project(n: &SyntaxNode) -> ProjectDecl {
    let name = first_ident(n);
    let profile = find_child(n, SyntaxKind::PROFILE_LIST)
        .map(|pl| {
            pl.children_with_tokens()
                .filter_map(|el| el.into_token())
                .filter(|t| t.kind() == SyntaxKind::IDENT)
                .map(|t| t.text().to_string())
                .collect()
        })
        .unwrap_or_default();

    ProjectDecl { name, profile }
}

fn lower_module(n: &SyntaxNode) -> ModuleDecl {
    let path = find_child(n, SyntaxKind::DOTTED_PATH)
        .map(|dp| lower_dotted_path(&dp))
        .unwrap_or_default();
    ModuleDecl { path }
}

fn lower_dotted_path(n: &SyntaxNode) -> Vec<String> {
    n.children_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| t.kind() == SyntaxKind::IDENT)
        .map(|t| t.text().to_string())
        .collect()
}

fn lower_import(n: &SyntaxNode) -> ImportDecl {
    let path = find_child(n, SyntaxKind::DOTTED_PATH)
        .map(|dp| lower_dotted_path(&dp))
        .unwrap_or_default();

    // alias: look for AS_KW followed by IDENT
    let mut alias = None;
    let mut saw_as = false;
    for el in n.children_with_tokens() {
        if let Some(tok) = el.as_token() {
            if tok.kind() == SyntaxKind::AS_KW {
                saw_as = true;
            } else if saw_as && tok.kind() == SyntaxKind::IDENT {
                alias = Some(tok.text().to_string());
                saw_as = false;
            }
        }
    }

    let items = find_child(n, SyntaxKind::IMPORT_ITEM_LIST)
        .map(|il| {
            il.children_with_tokens()
                .filter_map(|el| el.into_token())
                .filter(|t| t.kind() == SyntaxKind::IDENT)
                .map(|t| t.text().to_string())
                .collect()
        })
        .unwrap_or_default();

    ImportDecl { path, alias, items }
}

// -----------------------------------------------------------------
// Contract
// -----------------------------------------------------------------

fn lower_contract(n: &SyntaxNode) -> ContractDecl {
    let name = first_ident(n);
    let type_params = lower_type_params(n);
    let clauses: Vec<Clause> = n
        .children()
        .filter(|c| c.kind() == SyntaxKind::CLAUSE)
        .map(|c| lower_clause(&c))
        .collect();

    ContractDecl {
        name,
        type_params,
        clauses,
    }
}

// -----------------------------------------------------------------
// Clauses
// -----------------------------------------------------------------

fn lower_clause(n: &SyntaxNode) -> Clause {
    // First token is the clause keyword
    let kind = n
        .children_with_tokens()
        .find_map(|el| {
            let tok = el.into_token()?;
            clause_kind_from_syntax(tok.kind(), tok.text())
        })
        .unwrap_or(ClauseKind::Other("unknown".into()));

    // The body is everything after the keyword token
    let body = lower_clause_body(n);

    Clause { kind, body }
}

fn clause_kind_from_syntax(k: SyntaxKind, text: &str) -> Option<ClauseKind> {
    match k {
        SyntaxKind::REQUIRES_KW => Some(ClauseKind::Requires),
        SyntaxKind::ENSURES_KW => Some(ClauseKind::Ensures),
        SyntaxKind::EFFECTS_KW => Some(ClauseKind::Effects),
        SyntaxKind::INVARIANT_KW => Some(ClauseKind::Invariant),
        SyntaxKind::MODIFIES_KW => Some(ClauseKind::Modifies),
        SyntaxKind::INPUT_KW => Some(ClauseKind::Input),
        SyntaxKind::OUTPUT_KW => Some(ClauseKind::Output),
        SyntaxKind::ERRORS_KW => Some(ClauseKind::Errors),
        SyntaxKind::RULE_KW => Some(ClauseKind::Rule),
        SyntaxKind::DATA_FLOW_KW => Some(ClauseKind::DataFlow),
        SyntaxKind::MUST_NOT_KW => Some(ClauseKind::MustNot),
        SyntaxKind::DECREASES_KW => Some(ClauseKind::Decreases),
        SyntaxKind::GHOST_KW => Some(ClauseKind::Other("ghost".into())),
        SyntaxKind::DEFINE_KW => Some(ClauseKind::Other("define".into())),
        SyntaxKind::PROPERTY_KW => Some(ClauseKind::Other("property".into())),
        SyntaxKind::CONSTANT_TIME_KW => Some(ClauseKind::Other("constant_time".into())),
        SyntaxKind::MUST_BE_KW => Some(ClauseKind::Other("must_be".into())),
        SyntaxKind::VERIFY_AGAINST_KW => Some(ClauseKind::Other("verify_against".into())),
        SyntaxKind::READS_KW => Some(ClauseKind::Other("reads".into())),
        SyntaxKind::BOUNDS_KW => Some(ClauseKind::Other("bounds".into())),
        SyntaxKind::INTERFACE_KW => Some(ClauseKind::Other("interface".into())),
        SyntaxKind::EXTENDS_KW => Some(ClauseKind::Other("extends".into())),
        SyntaxKind::IMPL_KW => Some(ClauseKind::Other("implements".into())),
        SyntaxKind::IDENT => Some(ClauseKind::Other(text.to_string())),
        _ => None,
    }
}

/// Lower clause body: try to build an Expr from child nodes, fall back to raw tokens.
fn lower_clause_body(n: &SyntaxNode) -> Expr {
    // Look for expression nodes in children
    for child in n.children() {
        let k = child.kind();
        if is_expr_kind(k) {
            return lower_expr(&child);
        }
    }

    // Fall back to raw token collection.
    // Skip: the clause keyword, outer delimiters (parens/braces), whitespace.
    // Keep: colons inside the body (they separate param names from types),
    //       commas (they separate parameters), all other tokens.
    // The leading colon (separator between keyword and body) is also skipped.
    let mut saw_content = false;
    let tokens: Vec<String> = n
        .children_with_tokens()
        .skip(1) // skip clause keyword
        .filter_map(|el| match el {
            rowan::NodeOrToken::Token(t) => {
                let k = t.kind();
                if k == SyntaxKind::WHITESPACE || k == SyntaxKind::COMMENT {
                    return None;
                }
                // Skip outer delimiters
                if k == SyntaxKind::L_BRACE
                    || k == SyntaxKind::R_BRACE
                    || k == SyntaxKind::L_PAREN
                    || k == SyntaxKind::R_PAREN
                {
                    saw_content = true;
                    return None;
                }
                // Skip leading colon (keyword: body separator)
                if k == SyntaxKind::COLON && !saw_content {
                    return None;
                }
                saw_content = true;
                Some(t.text().to_string())
            }
            rowan::NodeOrToken::Node(n) => {
                saw_content = true;
                let texts = collect_token_texts(&n);
                if texts.is_empty() {
                    None
                } else {
                    Some(texts.join(" "))
                }
            }
        })
        .filter(|s| !s.is_empty())
        .collect();

    if tokens.is_empty() {
        Expr::Raw(vec![])
    } else {
        Expr::Raw(tokens)
    }
}

// -----------------------------------------------------------------
// Expressions
// -----------------------------------------------------------------

fn is_expr_kind(k: SyntaxKind) -> bool {
    matches!(
        k,
        SyntaxKind::LITERAL_EXPR
            | SyntaxKind::IDENT_EXPR
            | SyntaxKind::FIELD_EXPR
            | SyntaxKind::METHOD_CALL_EXPR
            | SyntaxKind::CALL_EXPR
            | SyntaxKind::INDEX_EXPR
            | SyntaxKind::BIN_EXPR
            | SyntaxKind::UNARY_EXPR
            | SyntaxKind::OLD_EXPR
            | SyntaxKind::FORALL_EXPR
            | SyntaxKind::EXISTS_EXPR
            | SyntaxKind::IF_EXPR
            | SyntaxKind::PAREN_EXPR
            | SyntaxKind::LIST_EXPR
            | SyntaxKind::CAST_EXPR
            | SyntaxKind::GHOST_EXPR
            | SyntaxKind::APPLY_EXPR
            | SyntaxKind::LET_EXPR
            | SyntaxKind::MATCH_EXPR
            | SyntaxKind::TUPLE_EXPR
            | SyntaxKind::RANGE_EXPR
            | SyntaxKind::RESULT_EXPR
            | SyntaxKind::SELF_EXPR
    )
}

fn lower_expr(n: &SyntaxNode) -> Expr {
    match n.kind() {
        SyntaxKind::LITERAL_EXPR => lower_literal(n),
        SyntaxKind::IDENT_EXPR => {
            let text = collect_text(n).trim().to_string();
            Expr::Ident(text)
        }
        SyntaxKind::SELF_EXPR => Expr::Ident("self".into()),
        SyntaxKind::RESULT_EXPR => Expr::Ident("result".into()),
        SyntaxKind::FIELD_EXPR => lower_field_expr(n),
        SyntaxKind::METHOD_CALL_EXPR => lower_method_call(n),
        SyntaxKind::CALL_EXPR => lower_call_expr(n),
        SyntaxKind::INDEX_EXPR => lower_index_expr(n),
        SyntaxKind::BIN_EXPR => lower_bin_expr(n),
        SyntaxKind::UNARY_EXPR => lower_unary_expr(n),
        SyntaxKind::OLD_EXPR => lower_old_expr(n),
        SyntaxKind::FORALL_EXPR => lower_quantifier(n, true),
        SyntaxKind::EXISTS_EXPR => lower_quantifier(n, false),
        SyntaxKind::IF_EXPR => lower_if_expr(n),
        SyntaxKind::PAREN_EXPR => {
            let inner = n.children().find_map(|c| {
                if is_expr_kind(c.kind()) {
                    Some(lower_expr(&c))
                } else {
                    None
                }
            });
            Expr::Paren(Box::new(inner.unwrap_or(Expr::Raw(vec![]))))
        }
        SyntaxKind::TUPLE_EXPR => {
            let items: Vec<Expr> = n
                .children()
                .filter(|c| is_expr_kind(c.kind()))
                .map(|c| lower_expr(&c))
                .collect();
            Expr::Tuple(items)
        }
        SyntaxKind::LIST_EXPR => {
            let items: Vec<Expr> = n
                .children()
                .filter(|c| is_expr_kind(c.kind()))
                .map(|c| lower_expr(&c))
                .collect();
            Expr::List(items)
        }
        SyntaxKind::CAST_EXPR => lower_cast_expr(n),
        SyntaxKind::GHOST_EXPR => {
            let inner = n.children().find_map(|c| {
                if is_expr_kind(c.kind()) {
                    Some(lower_expr(&c))
                } else {
                    None
                }
            });
            Expr::Ghost(Box::new(inner.unwrap_or(Expr::Raw(vec![]))))
        }
        SyntaxKind::APPLY_EXPR => lower_apply_expr(n),
        SyntaxKind::LET_EXPR => lower_let_expr(n),
        SyntaxKind::MATCH_EXPR => lower_match_expr(n),
        _ => {
            // Fallback: collect tokens as raw
            Expr::Raw(collect_token_texts(n))
        }
    }
}

fn lower_literal(n: &SyntaxNode) -> Expr {
    let Some(tok) = n.children_with_tokens().find_map(|el| el.into_token()) else {
        return Expr::Raw(collect_token_texts(n));
    };
    let text = tok.text().to_string();
    match tok.kind() {
        SyntaxKind::INT_LIT => Expr::Literal(Literal::Int(text)),
        SyntaxKind::FLOAT_LIT => Expr::Literal(Literal::Float(text)),
        SyntaxKind::STRING_LIT => Expr::Literal(Literal::Str(text)),
        SyntaxKind::TRUE_KW => Expr::Literal(Literal::Bool(true)),
        SyntaxKind::FALSE_KW => Expr::Literal(Literal::Bool(false)),
        _ => Expr::Literal(Literal::Str(text)),
    }
}

fn lower_field_expr(n: &SyntaxNode) -> Expr {
    let mut children_iter = n.children();
    let obj = children_iter
        .next()
        .map(|c| lower_expr(&c))
        .unwrap_or(Expr::Raw(vec![]));

    // Field name is the last IDENT or keyword token
    let field = n
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| t.kind() == SyntaxKind::IDENT || t.kind().is_keyword())
        .last()
        .map(|t| t.text().to_string())
        .unwrap_or_default();

    Expr::Field(Box::new(obj), field)
}

fn lower_method_call(n: &SyntaxNode) -> Expr {
    let mut children_iter = n.children();
    let receiver = children_iter
        .next()
        .map(|c| lower_expr(&c))
        .unwrap_or(Expr::Raw(vec![]));

    // Method name: IDENT or keyword token after DOT
    let method = n
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| t.kind() == SyntaxKind::IDENT || t.kind().is_keyword())
        .last()
        .map(|t| t.text().to_string())
        .unwrap_or_default();

    let args = find_child(n, SyntaxKind::ARG_LIST)
        .map(|al| lower_arg_list(&al))
        .unwrap_or_default();

    Expr::MethodCall {
        receiver: Box::new(receiver),
        method,
        args,
    }
}

fn lower_call_expr(n: &SyntaxNode) -> Expr {
    let func_name = first_ident(n);
    let args = find_child(n, SyntaxKind::ARG_LIST)
        .map(|al| lower_arg_list(&al))
        .unwrap_or_default();

    Expr::Call {
        func: Box::new(Expr::Ident(func_name)),
        args,
    }
}

fn lower_arg_list(n: &SyntaxNode) -> Vec<Expr> {
    n.children()
        .filter(|c| is_expr_kind(c.kind()))
        .map(|c| lower_expr(&c))
        .collect()
}

fn lower_index_expr(n: &SyntaxNode) -> Expr {
    let mut exprs = n.children().filter(|c| is_expr_kind(c.kind()));
    let base = exprs
        .next()
        .map(|c| lower_expr(&c))
        .unwrap_or(Expr::Raw(vec![]));
    let index = exprs
        .next()
        .map(|c| lower_expr(&c))
        .unwrap_or(Expr::Raw(vec![]));

    Expr::Index {
        expr: Box::new(base),
        index: Box::new(index),
    }
}

fn lower_bin_expr(n: &SyntaxNode) -> Expr {
    let mut exprs = n.children().filter(|c| is_expr_kind(c.kind()));
    let lhs = exprs
        .next()
        .map(|c| lower_expr(&c))
        .unwrap_or(Expr::Raw(vec![]));
    let rhs = exprs
        .next()
        .map(|c| lower_expr(&c))
        .unwrap_or(Expr::Raw(vec![]));

    // Find the operator token
    let op = n
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find_map(|t| bin_op_from_kind(t.kind()))
        .unwrap_or(BinOp::Add);

    Expr::BinOp {
        lhs: Box::new(lhs),
        op,
        rhs: Box::new(rhs),
    }
}

fn bin_op_from_kind(k: SyntaxKind) -> Option<BinOp> {
    match k {
        SyntaxKind::PLUS => Some(BinOp::Add),
        SyntaxKind::MINUS => Some(BinOp::Sub),
        SyntaxKind::STAR => Some(BinOp::Mul),
        SyntaxKind::SLASH => Some(BinOp::Div),
        SyntaxKind::PERCENT => Some(BinOp::Mod),
        SyntaxKind::EQ => Some(BinOp::Eq),
        SyntaxKind::NEQ => Some(BinOp::Neq),
        SyntaxKind::L_ANGLE => Some(BinOp::Lt),
        SyntaxKind::R_ANGLE => Some(BinOp::Gt),
        SyntaxKind::LTE => Some(BinOp::Lte),
        SyntaxKind::GTE => Some(BinOp::Gte),
        SyntaxKind::AND_AND | SyntaxKind::AND_KW => Some(BinOp::And),
        SyntaxKind::OR_OR | SyntaxKind::OR_KW => Some(BinOp::Or),
        SyntaxKind::FAT_ARROW => Some(BinOp::Implies),
        SyntaxKind::IN_KW => Some(BinOp::In),
        SyntaxKind::NOT_KW => Some(BinOp::NotIn), // `not in` — the `in` is consumed separately
        SyntaxKind::IS_KW => Some(BinOp::Eq),
        SyntaxKind::CONCAT => Some(BinOp::Concat),
        SyntaxKind::DOT_DOT => Some(BinOp::Range),
        _ => None,
    }
}

fn lower_unary_expr(n: &SyntaxNode) -> Expr {
    let inner = n
        .children()
        .find(|c| is_expr_kind(c.kind()))
        .map(|c| lower_expr(&c))
        .unwrap_or(Expr::Raw(vec![]));

    let op = n
        .children_with_tokens()
        .find_map(|el| {
            let tok = el.into_token()?;
            match tok.kind() {
                SyntaxKind::NOT_KW | SyntaxKind::BANG => Some(UnaryOp::Not),
                SyntaxKind::MINUS => Some(UnaryOp::Neg),
                _ => None,
            }
        })
        .unwrap_or(UnaryOp::Not);

    Expr::UnaryOp {
        op,
        expr: Box::new(inner),
    }
}

fn lower_old_expr(n: &SyntaxNode) -> Expr {
    let inner = n
        .children()
        .find(|c| is_expr_kind(c.kind()))
        .map(|c| lower_expr(&c))
        .unwrap_or(Expr::Raw(vec![]));
    Expr::Old(Box::new(inner))
}

fn lower_quantifier(n: &SyntaxNode, is_forall: bool) -> Expr {
    // forall/exists var in domain: body
    let idents: Vec<String> = n
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| t.kind() == SyntaxKind::IDENT)
        .map(|t| t.text().to_string())
        .collect();

    let var = idents.first().cloned().unwrap_or_default();

    let mut exprs = n.children().filter(|c| is_expr_kind(c.kind()));
    let domain = exprs
        .next()
        .map(|c| lower_expr(&c))
        .unwrap_or(Expr::Raw(vec![]));
    let body = exprs
        .next()
        .map(|c| lower_expr(&c))
        .unwrap_or(Expr::Raw(vec![]));

    if is_forall {
        Expr::Forall {
            var,
            domain: Box::new(domain),
            body: Box::new(body),
        }
    } else {
        Expr::Exists {
            var,
            domain: Box::new(domain),
            body: Box::new(body),
        }
    }
}

fn lower_if_expr(n: &SyntaxNode) -> Expr {
    let mut exprs = n.children().filter(|c| is_expr_kind(c.kind()));
    let cond = exprs
        .next()
        .map(|c| lower_expr(&c))
        .unwrap_or(Expr::Raw(vec![]));
    let then_branch = exprs
        .next()
        .map(|c| lower_expr(&c))
        .unwrap_or(Expr::Raw(vec![]));
    let else_branch = exprs.next().map(|c| Box::new(lower_expr(&c)));

    Expr::If {
        cond: Box::new(cond),
        then_branch: Box::new(then_branch),
        else_branch,
    }
}

fn lower_cast_expr(n: &SyntaxNode) -> Expr {
    let inner = n
        .children()
        .find(|c| is_expr_kind(c.kind()))
        .map(|c| lower_expr(&c))
        .unwrap_or(Expr::Raw(vec![]));

    // Type name: the token after `as`
    let mut saw_as = false;
    let ty = n
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| {
            if t.kind() == SyntaxKind::AS_KW {
                saw_as = true;
                return false;
            }
            saw_as && (t.kind() == SyntaxKind::IDENT || t.kind().is_keyword())
        })
        .map(|t| t.text().to_string())
        .unwrap_or_default();

    Expr::Cast {
        expr: Box::new(inner),
        ty,
    }
}

fn lower_apply_expr(n: &SyntaxNode) -> Expr {
    let lemma_name = first_ident(n);
    let args = find_child(n, SyntaxKind::ARG_LIST)
        .map(|al| lower_arg_list(&al))
        .unwrap_or_default();
    Expr::Apply { lemma_name, args }
}

fn lower_let_expr(n: &SyntaxNode) -> Expr {
    let name = first_ident(n);
    let mut exprs = n.children().filter(|c| is_expr_kind(c.kind()));
    let value = exprs
        .next()
        .map(|c| lower_expr(&c))
        .unwrap_or(Expr::Raw(vec![]));
    let body = exprs
        .next()
        .map(|c| lower_expr(&c))
        .unwrap_or(Expr::Raw(vec![]));

    Expr::Let {
        name,
        value: Box::new(value),
        body: Box::new(body),
    }
}

fn lower_match_expr(n: &SyntaxNode) -> Expr {
    let scrutinee = n
        .children()
        .find(|c| is_expr_kind(c.kind()))
        .map(|c| lower_expr(&c))
        .unwrap_or(Expr::Raw(vec![]));

    let arms = find_child(n, SyntaxKind::MATCH_ARM_LIST)
        .map(|al| {
            al.children()
                .filter(|c| c.kind() == SyntaxKind::MATCH_ARM)
                .map(|c| lower_match_arm(&c))
                .collect()
        })
        .unwrap_or_default();

    Expr::Match {
        scrutinee: Box::new(scrutinee),
        arms,
    }
}

fn lower_match_arm(n: &SyntaxNode) -> MatchArm {
    let pattern = n
        .children()
        .find_map(|c| lower_pattern(&c))
        .unwrap_or(Pattern::Wildcard);

    let body = n
        .children()
        .filter(|c| is_expr_kind(c.kind()))
        .last()
        .map(|c| lower_expr(&c))
        .unwrap_or(Expr::Raw(vec![]));

    MatchArm { pattern, body }
}

fn lower_pattern(n: &SyntaxNode) -> Option<Pattern> {
    match n.kind() {
        SyntaxKind::WILDCARD_PAT => Some(Pattern::Wildcard),
        SyntaxKind::IDENT_PAT => {
            let text = collect_text(n).trim().to_string();
            Some(Pattern::Ident(text))
        }
        SyntaxKind::LITERAL_PAT => {
            let tok = n.children_with_tokens().find_map(|el| el.into_token())?;
            match tok.kind() {
                SyntaxKind::INT_LIT => Some(Pattern::Literal(Literal::Int(tok.text().to_string()))),
                SyntaxKind::STRING_LIT => {
                    Some(Pattern::Literal(Literal::Str(tok.text().to_string())))
                }
                SyntaxKind::TRUE_KW => Some(Pattern::Literal(Literal::Bool(true))),
                SyntaxKind::FALSE_KW => Some(Pattern::Literal(Literal::Bool(false))),
                _ => None,
            }
        }
        SyntaxKind::CONSTRUCTOR_PAT => {
            let name = first_ident(n);
            let fields: Vec<Pattern> = n.children().filter_map(|c| lower_pattern(&c)).collect();
            Some(Pattern::Constructor { name, fields })
        }
        SyntaxKind::TUPLE_PAT => {
            let items: Vec<Pattern> = n.children().filter_map(|c| lower_pattern(&c)).collect();
            Some(Pattern::Tuple(items))
        }
        _ => None,
    }
}

// -----------------------------------------------------------------
// Type definitions
// -----------------------------------------------------------------

fn lower_type_def(n: &SyntaxNode) -> TypeDef {
    let name = first_ident(n);
    let type_params = lower_type_params(n);

    // Determine body type by looking at the structure
    let body = if has_token(n, SyntaxKind::EQUALS) {
        // Check for refined: = { ... }
        let has_braces_after_eq = {
            let mut saw_eq = false;
            n.children_with_tokens().any(|el| {
                if el.kind() == SyntaxKind::EQUALS {
                    saw_eq = true;
                }
                saw_eq && el.kind() == SyntaxKind::L_BRACE
            })
        };
        if has_braces_after_eq {
            // Refined: collect tokens inside the braces
            let tokens = collect_body_after_eq(n);
            TypeBody::Refined(tokens)
        } else {
            // Alias: collect tokens after =
            let tokens = collect_alias_tokens(n);
            TypeBody::Alias(tokens)
        }
    } else {
        // Struct body or empty
        let fields: Vec<FieldDef> = n
            .children()
            .filter(|c| c.kind() == SyntaxKind::FIELD_DEF)
            .map(|c| lower_field_def(&c))
            .collect();
        if fields.is_empty() {
            TypeBody::Empty
        } else {
            TypeBody::Struct(fields)
        }
    };

    TypeDef {
        name,
        type_params,
        body,
    }
}

fn collect_body_after_eq(n: &SyntaxNode) -> Vec<String> {
    let mut saw_eq = false;
    let mut inside_braces = false;
    let mut depth = 0i32;
    let mut tokens = Vec::new();

    for el in n.descendants_with_tokens() {
        if let Some(tok) = el.as_token() {
            if tok.kind() == SyntaxKind::EQUALS && !saw_eq {
                saw_eq = true;
                continue;
            }
            if !saw_eq {
                continue;
            }
            match tok.kind() {
                SyntaxKind::L_BRACE => {
                    if inside_braces {
                        depth += 1;
                        tokens.push("{".to_string());
                    } else {
                        inside_braces = true;
                    }
                }
                SyntaxKind::R_BRACE => {
                    if depth > 0 {
                        depth -= 1;
                        tokens.push("}".to_string());
                    } else {
                        // Closing brace of the outermost pair; stop.
                        inside_braces = false;
                    }
                }
                SyntaxKind::WHITESPACE | SyntaxKind::COMMENT => {}
                _ if inside_braces => {
                    tokens.push(tok.text().to_string());
                }
                _ => {}
            }
        }
    }
    tokens
}

fn collect_alias_tokens(n: &SyntaxNode) -> Vec<String> {
    let mut saw_eq = false;
    let mut tokens = Vec::new();

    for el in n.children_with_tokens() {
        if let Some(tok) = el.as_token() {
            if tok.kind() == SyntaxKind::EQUALS && !saw_eq {
                saw_eq = true;
                continue;
            }
            if !saw_eq {
                continue;
            }
            if tok.kind() == SyntaxKind::WHITESPACE || tok.kind() == SyntaxKind::COMMENT {
                continue;
            }
            if tok.kind() == SyntaxKind::SEMICOLON {
                break;
            }
            tokens.push(tok.text().to_string());
        }
    }
    tokens
}

fn lower_field_def(n: &SyntaxNode) -> FieldDef {
    let is_pub = has_token(n, SyntaxKind::PUB_KW);

    // Find field name: first IDENT that's not a modifier keyword
    let name = n
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| {
            t.kind() == SyntaxKind::IDENT
                && !matches!(t.text(), "var" | "ghost" | "pure" | "opaque")
        })
        .map(|t| t.text().to_string())
        .unwrap_or_default();

    // Type: everything after the colon
    let mut saw_colon = false;
    let ty: Vec<String> = n
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| {
            if t.kind() == SyntaxKind::COLON && !saw_colon {
                saw_colon = true;
                return false;
            }
            if !saw_colon {
                return false;
            }
            if matches!(
                t.kind(),
                SyntaxKind::SEMICOLON
                    | SyntaxKind::COMMA
                    | SyntaxKind::WHITESPACE
                    | SyntaxKind::COMMENT
            ) {
                return false;
            }
            true
        })
        .map(|t| t.text().to_string())
        .collect();

    let parsed_type = crate::ast::try_parse_type_tokens(&ty);
    FieldDef {
        name,
        ty,
        parsed_type,
        is_pub,
    }
}

// -----------------------------------------------------------------
// Enums
// -----------------------------------------------------------------

fn lower_enum_def(n: &SyntaxNode) -> EnumDef {
    let name = first_ident(n);
    let type_params = lower_type_params(n);
    let variants: Vec<EnumVariant> = n
        .children()
        .filter(|c| c.kind() == SyntaxKind::ENUM_VARIANT)
        .map(|c| lower_enum_variant(&c))
        .collect();

    EnumDef {
        name,
        type_params,
        variants,
    }
}

fn lower_enum_variant(n: &SyntaxNode) -> EnumVariant {
    let name = first_ident(n);
    // Fields: tokens inside parens (if any)
    let fields = collect_paren_tokens(n);
    EnumVariant { name, fields }
}

fn collect_paren_tokens(n: &SyntaxNode) -> Vec<String> {
    let mut inside = false;
    let mut depth = 0i32;
    let mut tokens = Vec::new();

    for el in n.children_with_tokens() {
        if let Some(tok) = el.as_token() {
            match tok.kind() {
                SyntaxKind::L_PAREN => {
                    if inside {
                        depth += 1;
                        tokens.push("(".to_string());
                    } else {
                        inside = true;
                    }
                }
                SyntaxKind::R_PAREN => {
                    if depth > 0 {
                        depth -= 1;
                        tokens.push(")".to_string());
                    } else {
                        break; // closing
                    }
                }
                SyntaxKind::WHITESPACE | SyntaxKind::COMMENT => {}
                _ if inside => {
                    tokens.push(tok.text().to_string());
                }
                _ => {}
            }
        }
    }
    tokens
}

// -----------------------------------------------------------------
// Extern
// -----------------------------------------------------------------

fn lower_extern(n: &SyntaxNode) -> ExternDecl {
    let name = first_ident(n);
    let params = lower_param_list(n);
    let return_ty = find_child(n, SyntaxKind::RETURN_TYPE)
        .map(|rt| collect_return_type_tokens(&rt))
        .unwrap_or_default();
    let clauses: Vec<Clause> = n
        .children()
        .filter(|c| c.kind() == SyntaxKind::CLAUSE)
        .map(|c| lower_clause(&c))
        .collect();

    let return_type_expr = crate::ast::try_parse_type_tokens(&return_ty);
    ExternDecl {
        name,
        params,
        return_ty,
        return_type_expr,
        clauses,
    }
}

// -----------------------------------------------------------------
// BindDecl
// -----------------------------------------------------------------

fn lower_bind(n: &SyntaxNode) -> BindDecl {
    // Extract the target path from the string literal token
    let target_path = n
        .children_with_tokens()
        .filter_map(|it| it.into_token())
        .find(|t| t.kind() == SyntaxKind::STRING_LIT)
        .map(|t| {
            let text = t.text().to_string();
            text.trim_matches('"').to_string()
        })
        .unwrap_or_default();

    let name = first_ident(n);

    // In bind declarations, params come from the `input(...)` clause
    // and the return type from the `output(...)` clause, not from
    // standalone PARAM_LIST / RETURN_TYPE nodes.
    let all_clauses: Vec<Clause> = n
        .children()
        .filter(|c| c.kind() == SyntaxKind::CLAUSE)
        .map(|c| lower_clause(&c))
        .collect();

    // Extract params from the input clause body (raw tokens like "a : Int , b : Int")
    let params = all_clauses
        .iter()
        .find(|c| c.kind == ClauseKind::Input)
        .map(|c| extract_params_from_clause_body(&c.body))
        .unwrap_or_default();

    // Extract return type from the output clause body
    let return_ty = all_clauses
        .iter()
        .find(|c| c.kind == ClauseKind::Output)
        .map(|c| extract_return_type_from_clause_body(&c.body))
        .unwrap_or_default();

    // Filter out input/output clauses; keep requires/ensures/effects etc.
    let clauses: Vec<Clause> = all_clauses
        .into_iter()
        .filter(|c| c.kind != ClauseKind::Input && c.kind != ClauseKind::Output)
        .collect();

    let return_type_expr = crate::ast::try_parse_type_tokens(&return_ty);
    BindDecl {
        name,
        target_path,
        params,
        return_ty,
        return_type_expr,
        clauses,
    }
}

/// Extract parameters from a clause body like `a : Int , b : Int`.
fn extract_params_from_clause_body(body: &Expr) -> Vec<Param> {
    let tokens = match body {
        Expr::Raw(toks) => toks,
        _ => return Vec::new(),
    };

    let mut params = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        // Skip commas
        if tokens[i] == "," {
            i += 1;
            continue;
        }
        let param_name = tokens[i].clone();
        i += 1;
        // Expect ":"
        if i < tokens.len() && tokens[i] == ":" {
            i += 1;
            // Collect type tokens until comma or end
            let mut ty = Vec::new();
            while i < tokens.len() && tokens[i] != "," {
                ty.push(tokens[i].clone());
                i += 1;
            }
            let parsed_type = crate::ast::try_parse_type_tokens(&ty);
            params.push(Param {
                name: param_name,
                ty,
                parsed_type,
            });
        } else {
            // Untyped param
            params.push(Param {
                name: param_name,
                ty: Vec::new(),
                parsed_type: None,
            });
        }
    }
    params
}

/// Extract return type tokens from a clause body like `result : Int`.
fn extract_return_type_from_clause_body(body: &Expr) -> Vec<String> {
    let tokens = match body {
        Expr::Raw(toks) => toks,
        _ => return Vec::new(),
    };
    // Skip "result :" prefix if present, take remaining type tokens
    if tokens.len() >= 2 && tokens[1] == ":" {
        tokens[2..].to_vec()
    } else {
        tokens.clone()
    }
}

// -----------------------------------------------------------------
// FnDef
// -----------------------------------------------------------------

fn lower_fn_def(n: &SyntaxNode) -> FnDef {
    let name = first_ident(n);

    // Check modifiers: only tokens BEFORE the fn/axiom/lemma keyword count.
    // Tokens inside the function body (e.g., `ghost { ... }`) must not
    // set these flags.
    let (is_ghost, is_lemma) = {
        let mut ghost = false;
        let mut lemma = false;
        for el in n.children_with_tokens() {
            let k = el.kind();
            // Stop once we hit the function keyword or the name
            if matches!(
                k,
                SyntaxKind::FN_KW | SyntaxKind::AXIOM_KW | SyntaxKind::LEMMA_KW
            ) {
                if k == SyntaxKind::LEMMA_KW {
                    lemma = true;
                }
                break;
            }
            if k == SyntaxKind::GHOST_KW {
                ghost = true;
            }
        }
        (ghost, lemma)
    };

    let params = lower_param_list(n);
    let return_ty = find_child(n, SyntaxKind::RETURN_TYPE)
        .map(|rt| collect_return_type_tokens(&rt))
        .unwrap_or_default();
    let clauses: Vec<Clause> = n
        .children()
        .filter(|c| c.kind() == SyntaxKind::CLAUSE)
        .map(|c| lower_clause(&c))
        .collect();

    let return_type_expr = crate::ast::try_parse_type_tokens(&return_ty);
    FnDef {
        name,
        is_ghost,
        is_lemma,
        params,
        return_ty,
        return_type_expr,
        clauses,
    }
}

// -----------------------------------------------------------------
// Service
// -----------------------------------------------------------------

fn lower_service(n: &SyntaxNode) -> ServiceDecl {
    let name = first_ident(n);
    let items: Vec<ServiceItem> = n
        .children()
        .filter_map(|c| lower_service_item(&c))
        .collect();
    ServiceDecl { name, items }
}

fn lower_service_item(n: &SyntaxNode) -> Option<ServiceItem> {
    match n.kind() {
        SyntaxKind::TYPE_DEF => Some(ServiceItem::TypeDef(lower_type_def(n))),
        SyntaxKind::ENUM_DEF => Some(ServiceItem::EnumDef(lower_enum_def(n))),
        SyntaxKind::SERVICE_ITEM => {
            // Determine the sub-kind from tokens
            let first_tok = n
                .children_with_tokens()
                .find_map(|el| el.into_token())
                .map(|t| t.kind());

            match first_tok {
                Some(SyntaxKind::STATES_KW) => {
                    let states: Vec<String> = n
                        .children_with_tokens()
                        .filter_map(|el| el.into_token())
                        .filter(|t| t.kind() == SyntaxKind::IDENT)
                        .map(|t| t.text().to_string())
                        .collect();
                    Some(ServiceItem::States(states))
                }
                Some(SyntaxKind::OPERATION_KW) => {
                    let name = first_ident(n);
                    let clauses: Vec<Clause> = n
                        .children()
                        .filter(|c| c.kind() == SyntaxKind::CLAUSE)
                        .map(|c| lower_clause(&c))
                        .collect();
                    Some(ServiceItem::Operation { name, clauses })
                }
                Some(SyntaxKind::QUERY_KW) => {
                    let name = first_ident(n);
                    let clauses: Vec<Clause> = n
                        .children()
                        .filter(|c| c.kind() == SyntaxKind::CLAUSE)
                        .map(|c| lower_clause(&c))
                        .collect();
                    Some(ServiceItem::Query { name, clauses })
                }
                Some(SyntaxKind::INVARIANT_KW) => {
                    let body = lower_clause_body(n);
                    Some(ServiceItem::Invariant(body))
                }
                _ => {
                    let kind = n
                        .children_with_tokens()
                        .find_map(|el| el.into_token())
                        .map(|t| t.text().to_string())
                        .unwrap_or_default();
                    let body = lower_clause_body(n);
                    Some(ServiceItem::Other { kind, body })
                }
            }
        }
        _ => None,
    }
}

// -----------------------------------------------------------------
// Generic block
// -----------------------------------------------------------------

fn lower_generic_block(n: &SyntaxNode) -> Decl {
    // First meaningful token is the kind
    let mut tokens_iter = n
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| t.kind() != SyntaxKind::WHITESPACE && t.kind() != SyntaxKind::COMMENT);

    let kind = tokens_iter
        .next()
        .map(|t| t.text().to_string())
        .unwrap_or_default();
    let name = tokens_iter
        .next()
        .filter(|t| t.kind() == SyntaxKind::IDENT || t.kind().is_keyword())
        .map(|t| t.text().to_string())
        .unwrap_or_default();

    let clauses: Vec<Clause> = n
        .children()
        .filter(|c| c.kind() == SyntaxKind::CLAUSE)
        .map(|c| lower_clause(&c))
        .collect();

    Decl::Block {
        kind,
        name,
        value: None, // TODO: extract inline values
        body: clauses,
    }
}

// -----------------------------------------------------------------
// Shared helpers
// -----------------------------------------------------------------

fn lower_type_params(n: &SyntaxNode) -> Vec<String> {
    find_child(n, SyntaxKind::TYPE_PARAM_LIST)
        .map(|tpl| {
            // Collect only param names (idents before colons), not bounds
            // (idents after colons). Format: Name [: Bound], Name [: Bound]
            let mut names = Vec::new();
            let mut in_bound = false;
            for el in tpl.children_with_tokens() {
                if let Some(tok) = el.as_token() {
                    match tok.kind() {
                        SyntaxKind::COLON => {
                            in_bound = true;
                        }
                        SyntaxKind::COMMA | SyntaxKind::R_ANGLE => {
                            in_bound = false;
                        }
                        SyntaxKind::IDENT if !in_bound => {
                            names.push(tok.text().to_string());
                        }
                        _ => {}
                    }
                }
            }
            names
        })
        .unwrap_or_default()
}

fn lower_param_list(n: &SyntaxNode) -> Vec<Param> {
    find_child(n, SyntaxKind::PARAM_LIST)
        .map(|pl| {
            pl.children()
                .filter(|c| c.kind() == SyntaxKind::PARAM)
                .map(|c| lower_param(&c))
                .collect()
        })
        .unwrap_or_default()
}

fn lower_param(n: &SyntaxNode) -> Param {
    // Name: first IDENT or keyword before colon
    let mut saw_colon = false;
    let mut name = String::new();
    let mut ty = Vec::new();

    for el in n.children_with_tokens() {
        match el {
            rowan::NodeOrToken::Token(tok) => {
                if tok.kind() == SyntaxKind::COLON && !saw_colon {
                    saw_colon = true;
                    continue;
                }
                if tok.kind() == SyntaxKind::WHITESPACE || tok.kind() == SyntaxKind::COMMENT {
                    continue;
                }
                if !saw_colon {
                    if name.is_empty()
                        && (tok.kind() == SyntaxKind::IDENT || tok.kind().is_keyword())
                    {
                        name = tok.text().to_string();
                    }
                } else {
                    ty.push(tok.text().to_string());
                }
            }
            rowan::NodeOrToken::Node(child) => {
                // Flatten child nodes (e.g., ATTR nodes) into type tokens
                if saw_colon {
                    let texts = collect_token_texts(&child);
                    ty.extend(texts);
                }
            }
        }
    }

    let parsed_type = crate::ast::try_parse_type_tokens(&ty);
    Param {
        name,
        ty,
        parsed_type,
    }
}

fn collect_return_type_tokens(n: &SyntaxNode) -> Vec<String> {
    // Skip the leading arrow/colon (the `->` or `:` separator),
    // then keep all remaining tokens including colons inside
    // refinement types like `{ v : Nat | ... }`.
    let mut skipped_leader = false;
    n.children_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(move |t| {
            if t.kind() == SyntaxKind::WHITESPACE || t.kind() == SyntaxKind::COMMENT {
                return false;
            }
            if !skipped_leader && (t.kind() == SyntaxKind::ARROW || t.kind() == SyntaxKind::COLON) {
                skipped_leader = true;
                return false;
            }
            true
        })
        .map(|t| t.text().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cst::{self, LexedToken, TokenSpan, build_tree};
    use crate::grammar;
    use crate::lexer::Token;
    use crate::syntax_kind::SyntaxKind;
    use logos::Logos;

    /// Parse source and lower to AST.
    fn parse_and_lower(source: &str) -> (SourceFile, Vec<cst::ParseError>) {
        let lex = Token::lexer(source);
        let mut tokens = Vec::new();
        let mut spans = Vec::new();

        for (tok, span) in lex.spanned() {
            if let Ok(t) = tok {
                tokens.push(LexedToken {
                    kind: SyntaxKind::from(&t),
                    text: source[span.clone()].to_string(),
                });
                spans.push(TokenSpan {
                    start: span.start,
                    end: span.end,
                });
            }
        }

        let mut parser = cst::Parser::new(tokens, spans);
        grammar::source_file(&mut parser);
        let green = build_tree(parser.events, &parser.tokens);
        let node = crate::syntax_kind::SyntaxNode::new_root(green);
        let sf = lower_source_file(&node);
        (sf, parser.errors)
    }

    #[test]
    fn lower_empty() {
        let (sf, errors) = parse_and_lower("");
        assert!(errors.is_empty());
        assert!(sf.project.is_none());
        assert!(sf.decls.is_empty());
    }

    #[test]
    fn lower_project_module_imports() {
        let src = r#"
            project MyApp {
                profile: [safety, security]
            }
            module mymod;
            import std.io;
        "#;
        let (sf, errors) = parse_and_lower(src);
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(sf.project.as_ref().unwrap().name, "MyApp");
        assert_eq!(
            sf.project.as_ref().unwrap().profile,
            vec!["safety", "security"]
        );
        assert_eq!(sf.module.as_ref().unwrap().path, vec!["mymod"]);
        assert_eq!(sf.imports.len(), 1);
        assert_eq!(sf.imports[0].path, vec!["std", "io"]);
    }

    #[test]
    fn lower_contract_with_clauses() {
        let src = r#"
            contract Foo {
                requires n > 0
                ensures result >= 0
            }
        "#;
        let (sf, errors) = parse_and_lower(src);
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(sf.decls.len(), 1);
        if let Decl::Contract(c) = &sf.decls[0].node {
            assert_eq!(c.name, "Foo");
            assert_eq!(c.clauses.len(), 2);
            assert_eq!(c.clauses[0].kind, ClauseKind::Requires);
            assert_eq!(c.clauses[1].kind, ClauseKind::Ensures);
        } else {
            panic!("expected Contract");
        }
    }

    #[test]
    fn lower_fn_with_clauses() {
        let src = r#"
            fn factorial(n: Nat) -> Nat
                requires n >= 0
                decreases n
                ensures result >= 1
        "#;
        let (sf, errors) = parse_and_lower(src);
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(sf.decls.len(), 1);
        if let Decl::FnDef(f) = &sf.decls[0].node {
            assert_eq!(f.name, "factorial");
            assert_eq!(f.params.len(), 1);
            assert_eq!(f.params[0].name, "n");
            assert_eq!(f.params[0].ty, vec!["Nat"]);
            assert_eq!(f.return_ty, vec!["Nat"]);
            assert_eq!(f.clauses.len(), 3);
        } else {
            panic!("expected FnDef");
        }
    }

    #[test]
    fn lower_type_struct() {
        let src = r#"
            type Point {
                x: Int;
                y: Int;
            }
        "#;
        let (sf, errors) = parse_and_lower(src);
        assert!(errors.is_empty(), "errors: {errors:?}");
        if let Decl::TypeDef(td) = &sf.decls[0].node {
            assert_eq!(td.name, "Point");
            if let TypeBody::Struct(fields) = &td.body {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].name, "x");
                assert_eq!(fields[1].name, "y");
            } else {
                panic!("expected Struct body");
            }
        } else {
            panic!("expected TypeDef");
        }
    }

    #[test]
    fn lower_enum() {
        let src = r#"
            enum Color {
                Red,
                Green,
                Blue,
            }
        "#;
        let (sf, errors) = parse_and_lower(src);
        assert!(errors.is_empty(), "errors: {errors:?}");
        if let Decl::EnumDef(ed) = &sf.decls[0].node {
            assert_eq!(ed.name, "Color");
            assert_eq!(ed.variants.len(), 3);
            assert_eq!(ed.variants[0].name, "Red");
        } else {
            panic!("expected EnumDef");
        }
    }

    #[test]
    fn lower_expr_binary() {
        let src = r#"
            contract Foo {
                requires a + b > 0
            }
        "#;
        let (sf, errors) = parse_and_lower(src);
        assert!(errors.is_empty(), "errors: {errors:?}");
        if let Decl::Contract(c) = &sf.decls[0].node {
            // The requires body should be a BinOp expression
            match &c.clauses[0].body {
                Expr::BinOp { op, .. } => assert_eq!(*op, BinOp::Gt),
                other => panic!("expected BinOp, got {other:?}"),
            }
        }
    }

    #[test]
    fn lower_bind_basic() {
        let src = r#"
            bind "std::cmp::max" as safe_max {
                input(a: Int, b: Int)
                output(result: Int)
                ensures result >= a
                ensures result >= b
            }
        "#;
        let (sf, errors) = parse_and_lower(src);
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(sf.decls.len(), 1);
        if let Decl::Bind(b) = &sf.decls[0].node {
            assert_eq!(b.name, "safe_max");
            assert_eq!(b.target_path, "std::cmp::max");
            assert_eq!(b.params.len(), 2);
            assert_eq!(b.params[0].name, "a");
            assert_eq!(b.params[1].name, "b");
            assert!(!b.return_ty.is_empty());
            assert_eq!(
                b.clauses.len(),
                2,
                "expected 2 ensures clauses, got {:?}",
                b.clauses
            );
            assert!(b.clauses.iter().all(|c| c.kind == ClauseKind::Ensures));
        } else {
            panic!("expected Decl::Bind, got {:?}", sf.decls[0].node);
        }
    }

    #[test]
    fn lower_bind_with_requires() {
        let src = r#"
            bind "my_crate::divide" as safe_divide {
                input(a: Int, b: Int)
                output(result: Int)
                requires b != 0
                ensures result * b == a
            }
        "#;
        let (sf, errors) = parse_and_lower(src);
        assert!(errors.is_empty(), "errors: {errors:?}");
        if let Decl::Bind(b) = &sf.decls[0].node {
            assert_eq!(b.name, "safe_divide");
            assert_eq!(b.target_path, "my_crate::divide");
            let requires_count = b
                .clauses
                .iter()
                .filter(|c| c.kind == ClauseKind::Requires)
                .count();
            let ensures_count = b
                .clauses
                .iter()
                .filter(|c| c.kind == ClauseKind::Ensures)
                .count();
            assert_eq!(requires_count, 1);
            assert_eq!(ensures_count, 1);
        } else {
            panic!("expected Decl::Bind");
        }
    }
}
