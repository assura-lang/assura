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
pub(crate) fn lower_source_file(root: &SyntaxNode) -> SourceFile {
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
                decls.push(spanned(Decl::Contract(lower_contract(&child)), &child))
            }
            SyntaxKind::SERVICE_DECL => {
                decls.push(spanned(Decl::Service(lower_service(&child)), &child))
            }
            SyntaxKind::TYPE_DEF => {
                decls.push(spanned(Decl::TypeDef(lower_type_def(&child)), &child))
            }
            SyntaxKind::ENUM_DEF => {
                decls.push(spanned(Decl::EnumDef(lower_enum_def(&child)), &child))
            }
            SyntaxKind::EXTERN_DECL => {
                decls.push(spanned(Decl::Extern(lower_extern(&child)), &child))
            }
            SyntaxKind::BIND_DECL => decls.push(spanned(Decl::Bind(lower_bind(&child)), &child)),
            SyntaxKind::PROPHECY_DECL => {
                decls.push(spanned(Decl::Prophecy(lower_prophecy(&child)), &child))
            }
            SyntaxKind::CODEC_REGISTRY_DECL => decls.push(spanned(
                Decl::CodecRegistry(lower_codec_registry(&child)),
                &child,
            )),
            SyntaxKind::FN_DEF => decls.push(spanned(Decl::FnDef(lower_fn_def(&child)), &child)),
            SyntaxKind::GENERIC_BLOCK => decls.push(spanned(lower_generic_block(&child), &child)),
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

/// Convenience: wrap a lowered node with the source span of the CST node.
fn spanned<T>(node: T, n: &SyntaxNode) -> Spanned<T> {
    Spanned {
        node,
        span: span_of(n),
    }
}

/// Sentinel for a missing sub-expression in recovery (e.g. bad receiver).
fn missing_expr() -> SpExpr {
    Spanned::no_span(Expr::Raw(vec![]))
}

/// Lower the first child expression (if any), else a recovery Raw.
fn lower_first_child_expr_or_missing(n: &SyntaxNode) -> SpExpr {
    n.children()
        .find(|c| is_expr_kind(c.kind()))
        .map(|c| lower_sp_expr(&c))
        .unwrap_or(missing_expr())
}

/// Lower a CST node into a `SpExpr` (expression with span).
fn lower_sp_expr(n: &SyntaxNode) -> SpExpr {
    lower_expr(n)
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
    first_token_text(n, |k, _| k == SyntaxKind::IDENT)
}

fn first_token_text<P>(n: &SyntaxNode, pred: P) -> String
where
    P: Fn(SyntaxKind, &str) -> bool,
{
    n.children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| pred(t.kind(), t.text()))
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

    let span = n.text_range();
    ImportDecl {
        path,
        alias,
        items,
        span: (span.start().into()..span.end().into()),
    }
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

    // Collect parameters from inline `fn` definitions inside the contract.
    // These params should be in scope for clause bodies (requires, ensures).
    let fn_params: Vec<Param> = n
        .children()
        .filter(|c| c.kind() == SyntaxKind::FN_DEF)
        .flat_map(|c| lower_param_list(&c))
        .collect();

    ContractDecl {
        name,
        type_params,
        clauses,
        fn_params,
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

    // For effects clauses, extract effect row variables (names after `|`)
    let effect_variables = if kind == ClauseKind::Effects {
        extract_effect_variables(&body.node)
    } else {
        vec![]
    };

    Clause {
        kind,
        body,
        effect_variables,
    }
}

/// Extract effect row variables from an effects clause body.
///
/// In `effects <io, net | E>`, the tokens after `|` that are capitalized
/// identifiers are effect row variables. This enables effect polymorphism
/// per Spec Section 1.12.
fn extract_effect_variables(body: &Expr) -> Vec<String> {
    let tokens = match body {
        Expr::Raw(tokens) => tokens,
        _ => return vec![],
    };

    let mut after_pipe = false;
    let mut vars = Vec::new();
    for tok in tokens {
        if tok == "|" {
            after_pipe = true;
        } else if after_pipe {
            let trimmed = tok.trim();
            if !trimmed.is_empty()
                && trimmed != ">"
                && trimmed != ","
                && trimmed.chars().next().is_some_and(|c| c.is_uppercase())
            {
                vars.push(trimmed.to_string());
            }
        }
    }
    vars
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
        SyntaxKind::CONFORMS_KW => Some(ClauseKind::Other("conforms".into())),
        SyntaxKind::ORDERING_KW => Some(ClauseKind::Ordering),
        SyntaxKind::TRUST_KW => Some(ClauseKind::Other("trust".into())),
        SyntaxKind::BOUNDARY_KW => Some(ClauseKind::Other("boundary".into())),
        SyntaxKind::MUST_PROPAGATE_KW => Some(ClauseKind::Other("must_propagate".into())),
        SyntaxKind::IDENT => Some(ClauseKind::Other(text.to_string())),
        // Map all other keyword tokens (domain-specific like circular_buffer,
        // deadline, etc.) to Other with their textual name. Without this,
        // they fall through to None and the NEXT token (an IDENT) is
        // mistakenly treated as the clause keyword.
        other if other.is_keyword() => Some(ClauseKind::Other(text.to_string())),
        _ => None,
    }
}

/// Lower clause body: try to build an Expr from child nodes, fall back to raw tokens.
fn lower_clause_body(n: &SyntaxNode) -> SpExpr {
    // Look for expression nodes (use descendants to find inner expr even
    // when wrapped in braces/parens/blocks for clause bodies like
    // `requires { 1 + true }`). This ensures the body SpExpr gets the
    // content's span (post 11.04).
    if let Some(expr_child) = n.descendants().find(|c| is_expr_kind(c.kind())) {
        return lower_sp_expr(&expr_child);
    }

    // Fall back to raw token collection.
    // Skip: the clause keyword (first significant token, which may be
    // preceded by trivia tokens attached to the CLAUSE node), outer
    // delimiters (parens/braces), whitespace.
    // Keep: colons inside the body (they separate param names from types),
    //       commas (they separate parameters), all other tokens.
    // The leading colon (separator between keyword and body) is also skipped.
    let mut saw_content = false;
    let mut skipped_kw = false;
    let tokens: Vec<String> = n
        .children_with_tokens()
        .filter_map(|el| match el {
            rowan::NodeOrToken::Token(t) => {
                let k = t.kind();
                if k == SyntaxKind::WHITESPACE || k == SyntaxKind::COMMENT {
                    return None;
                }
                // Skip the clause keyword (the first significant token under
                // this CLAUSE node). Trivia may precede it due to bump_trivia
                // on entry to clause().
                if !skipped_kw {
                    skipped_kw = true;
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

    let expr = if tokens.is_empty() {
        Expr::Raw(vec![])
    } else if tokens.len() == 1 && tokens[0].chars().all(|c| c.is_alphanumeric() || c == '_') {
        // Single identifier token: promote to Expr::Ident so downstream
        // checkers can pattern-match on Expr::Ident rather than Raw.
        Expr::Ident(
            tokens
                .into_iter()
                .next()
                .expect("tokens.len() == 1 guarantees at least one element"),
        )
    } else {
        Expr::Raw(tokens)
    };
    spanned(expr, n)
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

fn lower_expr(n: &SyntaxNode) -> SpExpr {
    match n.kind() {
        SyntaxKind::LITERAL_EXPR => spanned(lower_literal(n), n),
        SyntaxKind::IDENT_EXPR => {
            let text = collect_text(n).trim().to_string();
            spanned(Expr::Ident(text), n)
        }
        SyntaxKind::SELF_EXPR => spanned(Expr::Ident("self".into()), n),
        SyntaxKind::RESULT_EXPR => spanned(Expr::Ident("result".into()), n),
        SyntaxKind::FIELD_EXPR => lower_field_expr(n),
        SyntaxKind::METHOD_CALL_EXPR => lower_method_call(n),
        SyntaxKind::CALL_EXPR => lower_call_expr(n),
        SyntaxKind::INDEX_EXPR => lower_index_expr(n),
        SyntaxKind::BIN_EXPR => spanned(lower_bin_expr(n), n),
        SyntaxKind::UNARY_EXPR => spanned(lower_unary_expr(n), n),
        SyntaxKind::OLD_EXPR => spanned(lower_old_expr(n), n),
        SyntaxKind::FORALL_EXPR => spanned(lower_quantifier(n, true), n),
        SyntaxKind::EXISTS_EXPR => spanned(lower_quantifier(n, false), n),
        SyntaxKind::IF_EXPR => spanned(lower_if_expr(n), n),
        SyntaxKind::PAREN_EXPR => {
            let inner = n.children().find_map(|c| {
                if is_expr_kind(c.kind()) {
                    Some(lower_expr(&c))
                } else {
                    None
                }
            });
            inner.unwrap_or(spanned(Expr::Raw(vec![]), n))
        }
        SyntaxKind::TUPLE_EXPR => {
            let items = lower_expr_children(n);
            spanned(Expr::Tuple(items), n)
        }
        SyntaxKind::LIST_EXPR => {
            let items = lower_expr_children(n);
            spanned(Expr::List(items), n)
        }
        SyntaxKind::CAST_EXPR => spanned(lower_cast_expr(n), n),
        SyntaxKind::GHOST_EXPR => {
            let inner = n.children().find_map(|c| {
                if is_expr_kind(c.kind()) {
                    Some(lower_sp_expr(&c))
                } else {
                    None
                }
            });
            spanned(Expr::Ghost(Box::new(inner.unwrap_or(missing_expr()))), n)
        }
        SyntaxKind::APPLY_EXPR => spanned(lower_apply_expr(n), n),
        SyntaxKind::LET_EXPR => spanned(lower_let_expr(n), n),
        SyntaxKind::MATCH_EXPR => spanned(lower_match_expr(n), n),
        _ => {
            // Fallback: collect tokens as raw
            spanned(Expr::Raw(collect_token_texts(n)), n)
        }
    }
}

fn lower_literal(n: &SyntaxNode) -> Expr {
    let Some(tok) = n.children_with_tokens().filter_map(|el| el.into_token()).find(|t| !matches!(t.kind(), SyntaxKind::WHITESPACE | SyntaxKind::COMMENT)) else {
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

fn lower_field_expr(n: &SyntaxNode) -> SpExpr {
    let obj = lower_first_child_expr_or_missing(n);

    // Field name is the last IDENT or keyword token
    let field = n
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| t.kind() == SyntaxKind::IDENT || t.kind().is_keyword())
        .last()
        .map(|t| t.text().to_string())
        .unwrap_or_default();

    spanned(Expr::Field(Box::new(obj), field), n)
}

fn lower_method_call(n: &SyntaxNode) -> SpExpr {
    let receiver = lower_first_child_expr_or_missing(n);

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

    spanned(
        Expr::MethodCall {
            receiver: Box::new(receiver),
            method,
            args,
        },
        n,
    )
}

fn lower_call_expr(n: &SyntaxNode) -> SpExpr {
    // Function name: prefer IDENT, fall back to first keyword token text
    // (temporal operators like leads_to, eventually are keyword tokens).
    let func_name = first_ident_or_keyword(n);
    let mut args = find_child(n, SyntaxKind::ARG_LIST)
        .map(|al| lower_arg_list(&al))
        .unwrap_or_default();

    // For temporal operators with braced bodies (no ARG_LIST node),
    // collect child expressions directly as arguments.
    if args.is_empty() {
        let child_exprs = lower_expr_children(n);
        if !child_exprs.is_empty() {
            args = child_exprs;
        }
    }

    spanned(
        Expr::Call {
            func: Box::new(spanned(Expr::Ident(func_name), n)),
            args,
        },
        n,
    )
}

/// Get the first identifier or keyword text from a node's direct token children.
fn first_ident_or_keyword(n: &SyntaxNode) -> String {
    first_token_text(n, |k, _| k == SyntaxKind::IDENT || k.is_keyword())
}

fn lower_expr_children(n: &SyntaxNode) -> Vec<SpExpr> {
    n.children()
        .filter(|c| is_expr_kind(c.kind()))
        .map(|c| lower_sp_expr(&c))
        .collect()
}

fn lower_arg_list(n: &SyntaxNode) -> Vec<SpExpr> {
    lower_expr_children(n)
}

fn lower_index_expr(n: &SyntaxNode) -> SpExpr {
    let mut exprs = lower_expr_children(n).into_iter();
    let base = exprs.next().unwrap_or(missing_expr());
    let index = exprs.next().unwrap_or(missing_expr());

    spanned(
        Expr::Index {
            expr: Box::new(base),
            index: Box::new(index),
        },
        n,
    )
}

fn lower_bin_expr(n: &SyntaxNode) -> Expr {
    // Iteratively descend left-leaning BIN_EXPR chains to avoid stack
    // overflow on very long operator chains (e.g., 500+ chained &&).
    // The CST for `a && b && c` is left-recursive:
    //   BIN_EXPR(BIN_EXPR(a, &&, b), &&, c)
    // We collect (op, rhs) pairs walking down the left spine, then
    // build the AST bottom-up.
    let mut chain: Vec<(BinOp, SpExpr)> = Vec::new();
    let mut current = n.clone();

    loop {
        let mut exprs = current.children().filter(|c| is_expr_kind(c.kind()));
        let lhs_node = exprs.next();
        let rhs_node = exprs.next();

        let Some(op) = current
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .find_map(|t| bin_op_from_token(t.kind(), t.text()))
        else {
            let tokens: Vec<String> = current
                .children_with_tokens()
                .filter_map(|el| el.into_token())
                .map(|t| t.text().to_string())
                .collect();
            // Can't parse operator; return raw and apply any collected chain.
            let mut result = Spanned::no_span(Expr::Raw(tokens));
            result = apply_binop_chain(result, chain);
            return result.node;
        };

        let rhs = rhs_node
            .map(|c| lower_sp_expr(&c))
            .unwrap_or(missing_expr());

        chain.push((op, rhs));

        // If the LHS is itself a BIN_EXPR, continue iteratively instead
        // of recursing into lower_expr -> lower_bin_expr.
        match lhs_node {
            Some(lhs) if lhs.kind() == SyntaxKind::BIN_EXPR => {
                current = lhs;
            }
            _ => {
                // Base case: LHS is not a BIN_EXPR, lower it normally.
                let base = lhs_node
                    .map(|c| lower_sp_expr(&c))
                    .unwrap_or(missing_expr());
                let result = apply_binop_chain(base, chain);
                return result.node;
            }
        }
    }
}

/// Apply collected left-assoc binop chain to a base, building nested BinOp with combined spans.
/// Used to dedup the chain reconstruction logic in lower_bin_expr.
fn apply_binop_chain(mut result: SpExpr, chain: Vec<(BinOp, SpExpr)>) -> SpExpr {
    for (chain_op, chain_rhs) in chain.into_iter().rev() {
        let combined_span = result.span.start..chain_rhs.span.end;
        result = Spanned {
            node: Expr::BinOp {
                lhs: Box::new(result),
                op: chain_op,
                rhs: Box::new(chain_rhs),
            },
            span: combined_span,
        };
    }
    result
}

fn bin_op_from_token(k: SyntaxKind, text: &str) -> Option<BinOp> {
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
        SyntaxKind::IDENT if text == "mod" => Some(BinOp::Mod),
        _ => None,
    }
}

fn lower_unary_expr(n: &SyntaxNode) -> Expr {
    let inner = lower_first_child_expr_or_missing(n);

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
    let inner = lower_first_child_expr_or_missing(n);
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

    let mut exprs = lower_expr_children(n).into_iter();
    let domain = exprs.next().unwrap_or(missing_expr());
    let body = exprs.next().unwrap_or(missing_expr());

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
    let mut exprs = lower_expr_children(n).into_iter();
    let cond = exprs.next().unwrap_or(missing_expr());
    let then_branch = exprs.next().unwrap_or(missing_expr());
    let else_branch = exprs.next().map(Box::new);

    Expr::If {
        cond: Box::new(cond),
        then_branch: Box::new(then_branch),
        else_branch,
    }
}

fn lower_cast_expr(n: &SyntaxNode) -> Expr {
    let inner = lower_first_child_expr_or_missing(n);

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
    let args: Vec<SpExpr> = find_child(n, SyntaxKind::ARG_LIST)
        .map(|al| lower_arg_list(&al))
        .unwrap_or_default();
    Expr::Apply { lemma_name, args }
}

fn lower_let_expr(n: &SyntaxNode) -> Expr {
    let name = first_ident(n);
    let mut exprs = lower_expr_children(n).into_iter();
    let value = exprs.next().unwrap_or(missing_expr());
    let body = exprs.next().unwrap_or(missing_expr());

    Expr::Let {
        name,
        value: Box::new(value),
        body: Box::new(body),
    }
}

fn lower_match_expr(n: &SyntaxNode) -> Expr {
    let scrutinee = lower_first_child_expr_or_missing(n);

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

    let exprs = lower_expr_children(n);
    let body = exprs.into_iter().last().unwrap_or(missing_expr());

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

    let parsed = crate::ast::try_parse_type_tokens(&ty);
    FieldDef {
        name,
        ty: parsed,
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
                SyntaxKind::COMMA if inside && depth == 0 => {
                    // Skip top-level commas (field separators)
                }
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

    let return_ty = crate::ast::try_parse_type_tokens(&return_ty);
    ExternDecl {
        name,
        params,
        return_ty,
        clauses,
    }
}

// -----------------------------------------------------------------
// BindDecl
// -----------------------------------------------------------------

fn lower_prophecy(n: &SyntaxNode) -> ProphecyDecl {
    // Skip ghost, prophecy keywords; find the name (first IDENT)
    let name = first_ident(n);
    // Collect type tokens after ':'
    let mut ty_tokens = Vec::new();
    let mut after_colon = false;
    for elem in n.children_with_tokens() {
        if let Some(tok) = elem.as_token() {
            if tok.kind() == SyntaxKind::COLON {
                after_colon = true;
                continue;
            }
            if after_colon && tok.kind() != SyntaxKind::WHITESPACE {
                ty_tokens.push(tok.text().to_string());
            }
        }
    }
    // Only parse type if a colon was found (has type annotation)
    let ty = if after_colon {
        crate::ast::try_parse_type_tokens(&ty_tokens)
    } else {
        None
    };
    ProphecyDecl { name, ty }
}

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
        .map(|c| extract_params_from_clause_body(&c.body.node))
        .unwrap_or_default();

    // Extract return type from the output clause body
    let return_ty = all_clauses
        .iter()
        .find(|c| c.kind == ClauseKind::Output)
        .map(|c| extract_return_type_from_clause_body(&c.body.node))
        .unwrap_or_default();

    // Filter out input/output clauses; keep requires/ensures/effects etc.
    let clauses: Vec<Clause> = all_clauses
        .into_iter()
        .filter(|c| c.kind != ClauseKind::Input && c.kind != ClauseKind::Output)
        .collect();

    let return_ty = crate::ast::try_parse_type_tokens(&return_ty);
    BindDecl {
        name,
        target_path,
        params,
        return_ty,
        clauses,
    }
}

fn lower_codec_registry(n: &SyntaxNode) -> CodecRegistryDecl {
    let name = first_ident(n);

    // Collect all non-whitespace tokens from the CODEC_REGISTRY_DECL node.
    // We'll walk them to extract output_type and codec entries.
    let tokens: Vec<(SyntaxKind, String)> = n
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| {
            let k = t.kind();
            k != SyntaxKind::WHITESPACE && k != SyntaxKind::COMMENT
        })
        .map(|t| (t.kind(), t.text().to_string()))
        .collect();

    // Extract output type: tokens between "output" ":" and the first ","
    let mut output_type = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i].0 == SyntaxKind::OUTPUT_KW {
            i += 1; // skip "output"
            if i < tokens.len() && tokens[i].1 == ":" {
                i += 1; // skip ":"
            }
            while i < tokens.len() && tokens[i].1 != "," && tokens[i].0 != SyntaxKind::CODEC_KW {
                output_type.push(tokens[i].1.clone());
                i += 1;
            }
            break;
        }
        i += 1;
    }

    // Extract codec entries from child CODEC_ENTRY nodes
    let codecs: Vec<CodecEntry> = n
        .children()
        .filter(|c| c.kind() == SyntaxKind::CODEC_ENTRY)
        .map(|c| lower_codec_entry(&c))
        .collect();

    CodecRegistryDecl {
        name,
        output_type,
        codecs,
    }
}

fn lower_codec_entry(n: &SyntaxNode) -> CodecEntry {
    let name = first_ident(n);

    let tokens: Vec<(SyntaxKind, String)> = n
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| {
            let k = t.kind();
            k != SyntaxKind::WHITESPACE && k != SyntaxKind::COMMENT
        })
        .map(|t| (t.kind(), t.text().to_string()))
        .collect();

    let mut magic = MagicPattern::Bytes {
        bytes: Vec::new(),
        prefix: false,
    };
    let mut decoder = String::new();

    let mut i = 0;
    while i < tokens.len() {
        // magic: [...]  or  magic: extension(...)  or  magic: probe(...)
        if tokens[i].0 == SyntaxKind::MAGIC_KW {
            i += 1; // skip "magic"
            if i < tokens.len() && tokens[i].1 == ":" {
                i += 1; // skip ":"
            }
            if i < tokens.len() && tokens[i].1 == "[" {
                // BytePattern
                i += 1; // skip "["
                let mut bytes = Vec::new();
                let mut prefix = false;
                while i < tokens.len() && tokens[i].1 != "]" {
                    let t = &tokens[i].1;
                    if t == "," {
                        i += 1;
                        continue;
                    }
                    if t == ".." {
                        prefix = true;
                        i += 1;
                        continue;
                    }
                    // The lexer splits "0x89" into Int("0") + Ident("x89").
                    // Check for this two-token pattern first.
                    if t == "0" && i + 1 < tokens.len() && tokens[i + 1].1.starts_with(['x', 'X']) {
                        let hex_str = &tokens[i + 1].1[1..]; // skip 'x'
                        if let Ok(b) = u8::from_str_radix(hex_str, 16) {
                            bytes.push(b);
                        }
                        i += 2;
                        continue;
                    }
                    // Single-token hex: 0x89 (if lexer keeps it whole)
                    if let Some(stripped) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                        if let Ok(b) = u8::from_str_radix(stripped, 16) {
                            bytes.push(b);
                        }
                    } else if let Ok(b) = t.parse::<u8>() {
                        bytes.push(b);
                    }
                    i += 1;
                }
                magic = MagicPattern::Bytes { bytes, prefix };
            } else if i < tokens.len() && tokens[i].1 == "extension" {
                i += 1; // skip "extension"
                if i < tokens.len() && tokens[i].1 == "(" {
                    i += 1; // skip "("
                }
                let mut exts = Vec::new();
                while i < tokens.len() && tokens[i].1 != ")" {
                    let t = &tokens[i].1;
                    if t != "," {
                        exts.push(t.trim_matches('"').to_string());
                    }
                    i += 1;
                }
                magic = MagicPattern::Extension(exts);
            } else if i < tokens.len() && tokens[i].1 == "probe" {
                i += 1; // skip "probe"
                if i < tokens.len() && tokens[i].1 == "(" {
                    i += 1; // skip "("
                }
                let fn_name = if i < tokens.len() && tokens[i].1 != ")" {
                    let n = tokens[i].1.clone();
                    i += 1;
                    n
                } else {
                    String::new()
                };
                magic = MagicPattern::Probe(fn_name);
            }
        }

        // decoder: fn_name
        if i < tokens.len() && tokens[i].1 == "decoder" {
            i += 1; // skip "decoder"
            if i < tokens.len() && tokens[i].1 == ":" {
                i += 1; // skip ":"
            }
            if i < tokens.len()
                && tokens[i].1 != ","
                && tokens[i].1 != "}"
                && tokens[i].1 != "contracts"
            {
                decoder = tokens[i].1.clone();
            }
        }

        i += 1;
    }

    // Extract contracts from CLAUSE child nodes
    let contracts: Vec<Clause> = n
        .children()
        .filter(|c| c.kind() == SyntaxKind::CLAUSE)
        .map(|c| lower_clause(&c))
        .collect();

    CodecEntry {
        name,
        magic,
        decoder,
        contracts,
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
            let parsed = crate::ast::try_parse_type_tokens(&ty);
            params.push(Param {
                name: param_name,
                ty: parsed,
            });
        } else {
            // Untyped param
            params.push(Param {
                name: param_name,
                ty: None,
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

    let return_ty = crate::ast::try_parse_type_tokens(&return_ty);
    FnDef {
        name,
        is_ghost,
        is_lemma,
        params,
        return_ty,
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

    let kind_str = tokens_iter
        .next()
        .map(|t| t.text().to_string())
        .unwrap_or_default();
    let kind = BlockKind::from_keyword(&kind_str);
    let name = tokens_iter
        .next()
        .filter(|t| t.kind() == SyntaxKind::IDENT || t.kind().is_keyword())
        .map(|t| t.text().to_string())
        .unwrap_or_default();

    // Collect remaining tokens as the inline value (e.g., ": Nat = 280").
    // These are the tokens between the name and any brace-delimited body.
    let value_tokens: Vec<String> = tokens_iter
        .take_while(|t| t.kind() != SyntaxKind::L_BRACE && t.kind() != SyntaxKind::R_BRACE)
        .map(|t| t.text().to_string())
        .collect();
    let value = if value_tokens.is_empty() {
        None
    } else {
        Some(value_tokens)
    };

    let clauses: Vec<Clause> = n
        .children()
        .filter(|c| c.kind() == SyntaxKind::CLAUSE)
        .map(|c| lower_clause(&c))
        .collect();

    Decl::Block {
        kind,
        name,
        value,
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

    // Only parse type if a colon was found (has type annotation)
    let parsed = if saw_colon {
        crate::ast::try_parse_type_tokens(&ty)
    } else {
        None
    };
    Param { name, ty: parsed }
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
    fn lower_contract_with_inline_fn_params() {
        let src = r#"
            contract Bad {
                requires x > 0
                fn bad(x: Int, y: Float) -> Int
            }
        "#;
        let (sf, errors) = parse_and_lower(src);
        assert!(errors.is_empty(), "errors: {errors:?}");
        assert_eq!(sf.decls.len(), 1);
        if let Decl::Contract(c) = &sf.decls[0].node {
            assert_eq!(c.name, "Bad");
            assert_eq!(
                c.fn_params.len(),
                2,
                "fn_params should have 2 params, got: {:?}",
                c.fn_params
            );
            assert_eq!(c.fn_params[0].name, "x");
            assert_eq!(c.fn_params[1].name, "y");
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
            assert_eq!(
                f.params[0].ty,
                Some(crate::ast::TypeExpr::Named("Nat".into()))
            );
            assert_eq!(f.return_ty, Some(crate::ast::TypeExpr::Named("Nat".into())));
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
    fn lower_enum_variant_fields_exclude_commas() {
        let src = "enum Shape { Rect(Int, Int), Circle(Float) }";
        let (sf, errors) = parse_and_lower(src);
        assert!(errors.is_empty(), "errors: {errors:?}");
        if let Decl::EnumDef(ed) = &sf.decls[0].node {
            // Rect(Int, Int) should have exactly 2 fields, no commas
            assert_eq!(ed.variants[0].name, "Rect");
            assert_eq!(ed.variants[0].fields, vec!["Int", "Int"]);
            // Circle(Float) should have exactly 1 field
            assert_eq!(ed.variants[1].name, "Circle");
            assert_eq!(ed.variants[1].fields, vec!["Float"]);
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
            match &c.clauses[0].body.node {
                Expr::BinOp { op, .. } => assert_eq!(*op, BinOp::Gt),
                other => panic!("expected BinOp, got {other:?}"),
            }
        }
    }

    #[test]
    fn lower_expr_binary_chain_spans() {
        // For `a + b + c`, the inner `a + b` should have a span covering
        // both `a` and `b`, not 0..0.
        let src = r#"
            contract Foo {
                requires a + b + c > 0
            }
        "#;
        let (sf, errors) = parse_and_lower(src);
        assert!(errors.is_empty(), "errors: {errors:?}");
        if let Decl::Contract(c) = &sf.decls[0].node {
            // Top-level: BinOp(>, lhs=BinOp(+, ..., c), rhs=0)
            let body = &c.clauses[0].body;
            assert!(
                body.span != (0..0),
                "top-level body span should not be 0..0"
            );
            if let Expr::BinOp { lhs, .. } = &body.node {
                // lhs is `a + b + c` addition chain
                assert!(lhs.span != (0..0), "addition chain span should not be 0..0");
                if let Expr::BinOp {
                    lhs: inner_lhs,
                    rhs: inner_rhs,
                    ..
                } = &lhs.node
                {
                    // inner_lhs is `a + b`, inner_rhs is `c`
                    assert!(
                        inner_lhs.span != (0..0),
                        "inner `a + b` span should not be 0..0, got {:?}",
                        inner_lhs.span
                    );
                    assert!(
                        inner_rhs.span != (0..0),
                        "inner `c` span should not be 0..0, got {:?}",
                        inner_rhs.span
                    );
                }
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
            assert!(b.return_ty.is_some());
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

    #[test]
    fn test_prophecy_decl() {
        let src = "ghost prophecy future_value: Int";
        let (file, errs) = crate::parse(src);
        assert!(errs.is_empty(), "unexpected errors: {errs:?}");
        let file = file.unwrap();
        assert_eq!(file.decls.len(), 1);
        if let Decl::Prophecy(p) = &file.decls[0].node {
            assert_eq!(p.name, "future_value");
            assert_eq!(p.ty, Some(crate::ast::TypeExpr::Named("Int".into())));
        } else {
            panic!("expected Decl::Prophecy, got {:?}", file.decls[0].node);
        }
    }

    #[test]
    fn test_prophecy_decl_no_type() {
        let src = "ghost prophecy x";
        let (file, errs) = crate::parse(src);
        assert!(errs.is_empty(), "unexpected errors: {errs:?}");
        let file = file.unwrap();
        assert_eq!(file.decls.len(), 1);
        if let Decl::Prophecy(p) = &file.decls[0].node {
            assert_eq!(p.name, "x");
            assert!(p.ty.is_none());
        } else {
            panic!("expected Decl::Prophecy, got {:?}", file.decls[0].node);
        }
    }

    #[test]
    fn test_prophecy_with_contract() {
        let src = r#"
ghost prophecy final_result: Int

contract UseProphecy {
    requires { final_result > 0 }
}
"#;
        let (file, errs) = crate::parse(src);
        assert!(errs.is_empty(), "unexpected errors: {errs:?}");
        let file = file.unwrap();
        assert_eq!(file.decls.len(), 2);
        assert!(matches!(&file.decls[0].node, Decl::Prophecy(_)));
        assert!(matches!(&file.decls[1].node, Decl::Contract(_)));
    }

    #[test]
    fn test_consecutive_prophecy_declarations() {
        // Regression test for #158: consecutive prophecy decls merged into one block
        let src = "module test\nprophecy alpha: Int\nprophecy beta: Int";
        let (file, errs) = crate::parse(src);
        assert!(errs.is_empty(), "unexpected errors: {errs:?}");
        let file = file.unwrap();
        // Must produce TWO separate Prophecy decls, not one merged Block
        let prophecies: Vec<_> = file
            .decls
            .iter()
            .filter(|d| matches!(&d.node, Decl::Prophecy(_)))
            .collect();
        assert_eq!(
            prophecies.len(),
            2,
            "expected 2 prophecy decls, got {}: {:#?}",
            prophecies.len(),
            file.decls
                .iter()
                .map(|d| format!("{:?}", d.node))
                .collect::<Vec<_>>()
        );
        if let Decl::Prophecy(p) = &prophecies[0].node {
            assert_eq!(p.name, "alpha");
        }
        if let Decl::Prophecy(p) = &prophecies[1].node {
            assert_eq!(p.name, "beta");
        }
    }

    #[test]
    fn test_consecutive_ghost_prophecy_declarations() {
        // ghost prophecy form should also work consecutively
        let src = "ghost prophecy a: Int\nghost prophecy b: Float";
        let (file, errs) = crate::parse(src);
        assert!(errs.is_empty(), "unexpected errors: {errs:?}");
        let file = file.unwrap();
        let prophecies: Vec<_> = file
            .decls
            .iter()
            .filter(|d| matches!(&d.node, Decl::Prophecy(_)))
            .collect();
        assert_eq!(prophecies.len(), 2);
        if let Decl::Prophecy(p) = &prophecies[0].node {
            assert_eq!(p.name, "a");
        }
        if let Decl::Prophecy(p) = &prophecies[1].node {
            assert_eq!(p.name, "b");
        }
    }

    #[test]
    fn test_liveness_block() {
        let src = r#"
liveness Progress {
    assume: fair
    prove: eventually(turn == 1)
}
"#;
        let (file, errs) = crate::parse(src);
        assert!(errs.is_empty(), "unexpected errors: {errs:?}");
        let file = file.unwrap();
        assert_eq!(file.decls.len(), 1);
        if let Decl::Block {
            kind, name, body, ..
        } = &file.decls[0].node
        {
            assert_eq!(*kind, BlockKind::Liveness);
            assert_eq!(name, "Progress");
            assert!(
                body.len() >= 2,
                "expected assume + prove clauses, got {}",
                body.len()
            );
        } else {
            panic!("expected Decl::Block, got {:?}", file.decls[0].node);
        }
    }

    #[test]
    fn test_liveness_block_braced_body() {
        // Regression test for #53: clause bodies with braces inside generic blocks
        let src = r#"
liveness Progress {
    assume: fair
    prove: eventually { turn == 1 }
}
"#;
        let (file, errs) = crate::parse(src);
        assert!(errs.is_empty(), "unexpected errors: {errs:?}");
        let file = file.unwrap();
        assert_eq!(file.decls.len(), 1);
        if let Decl::Block {
            kind, name, body, ..
        } = &file.decls[0].node
        {
            assert_eq!(*kind, BlockKind::Liveness);
            assert_eq!(name, "Progress");
            assert!(
                body.len() >= 2,
                "expected assume + prove clauses, got {}",
                body.len()
            );
        } else {
            panic!("expected Decl::Block, got {:?}", file.decls[0].node);
        }
    }

    #[test]
    fn test_liveness_block_multiple_braced_clauses() {
        // Also covers #53: multiple brace-delimited bodies in one block
        let src = r#"
liveness Fairness {
    prove: eventually { turn == 1 }
    prove: eventually_within { progress == true }
}
"#;
        let (file, errs) = crate::parse(src);
        assert!(errs.is_empty(), "unexpected errors: {errs:?}");
        let file = file.unwrap();
        if let Decl::Block { body, .. } = &file.decls[0].node {
            assert!(
                body.len() >= 2,
                "expected 2 prove clauses, got {}",
                body.len()
            );
        } else {
            panic!("expected Decl::Block");
        }
    }

    #[test]
    fn test_generic_block_value_extraction() {
        let src = "feature_max MAX_SIZE: Nat = 256";
        let (file, errs) = crate::parse(src);
        assert!(errs.is_empty(), "unexpected errors: {errs:?}");
        let file = file.unwrap();
        assert_eq!(file.decls.len(), 1);
        if let Decl::Block {
            kind, name, value, ..
        } = &file.decls[0].node
        {
            assert_eq!(*kind, BlockKind::FeatureMax);
            assert_eq!(name, "MAX_SIZE");
            let v = value.as_ref().expect("value should be Some");
            assert!(
                v.contains(&"256".to_string()),
                "value tokens should contain '256', got: {v:?}"
            );
        } else {
            panic!("expected Decl::Block, got {:?}", file.decls[0].node);
        }
    }

    #[test]
    fn test_generic_block_equals_value() {
        let src = "feature ecdsa = enabled";
        let (file, errs) = crate::parse(src);
        assert!(errs.is_empty(), "unexpected errors: {errs:?}");
        let file = file.unwrap();
        assert_eq!(file.decls.len(), 1);
        if let Decl::Block {
            kind, name, value, ..
        } = &file.decls[0].node
        {
            assert_eq!(*kind, BlockKind::Feature);
            assert_eq!(name, "ecdsa");
            let v = value.as_ref().expect("value should be Some");
            assert!(
                v.contains(&"enabled".to_string()),
                "value tokens should contain 'enabled', got: {v:?}"
            );
        } else {
            panic!("expected Decl::Block, got {:?}", file.decls[0].node);
        }
    }

    #[test]
    fn lower_codec_registry_basic() {
        let src = r#"
            codec_registry ImageFormats {
                output: ImageOutput,

                codec Png {
                    magic: [0x89, 0x50, 0x4E, 0x47],
                    decoder: decode_png
                }

                codec Bmp {
                    magic: [0x42, 0x4D, ..],
                    decoder: decode_bmp
                }
            }
        "#;
        let file = crate::parse_unwrap(src);
        assert_eq!(file.decls.len(), 1);
        if let Decl::CodecRegistry(cr) = &file.decls[0].node {
            assert_eq!(cr.name, "ImageFormats");
            assert_eq!(cr.output_type, vec!["ImageOutput"]);
            assert_eq!(cr.codecs.len(), 2);

            assert_eq!(cr.codecs[0].name, "Png");
            assert_eq!(cr.codecs[0].decoder, "decode_png");
            if let MagicPattern::Bytes { bytes, prefix } = &cr.codecs[0].magic {
                assert_eq!(bytes, &[0x89, 0x50, 0x4E, 0x47]);
                assert!(!prefix);
            } else {
                panic!("expected MagicPattern::Bytes for Png");
            }

            assert_eq!(cr.codecs[1].name, "Bmp");
            assert_eq!(cr.codecs[1].decoder, "decode_bmp");
            if let MagicPattern::Bytes { bytes, prefix } = &cr.codecs[1].magic {
                assert_eq!(bytes, &[0x42, 0x4D]);
                assert!(prefix, "BMP should have prefix=true due to '..'");
            } else {
                panic!("expected MagicPattern::Bytes for Bmp");
            }
        } else {
            panic!("expected Decl::CodecRegistry, got {:?}", file.decls[0].node);
        }
    }

    #[test]
    fn lower_codec_registry_probe_and_extension() {
        let src = r#"
            codec_registry Formats {
                output: FormatOutput,

                codec Hdr {
                    magic: probe(is_hdr_format),
                    decoder: decode_hdr
                }

                codec Text {
                    magic: extension(".txt", ".md"),
                    decoder: decode_text
                }
            }
        "#;
        let file = crate::parse_unwrap(src);
        if let Decl::CodecRegistry(cr) = &file.decls[0].node {
            assert_eq!(cr.codecs.len(), 2);

            if let MagicPattern::Probe(fn_name) = &cr.codecs[0].magic {
                assert_eq!(fn_name, "is_hdr_format");
            } else {
                panic!("expected MagicPattern::Probe for Hdr");
            }

            if let MagicPattern::Extension(exts) = &cr.codecs[1].magic {
                assert_eq!(exts, &[".txt", ".md"]);
            } else {
                panic!("expected MagicPattern::Extension for Text");
            }
        } else {
            panic!("expected Decl::CodecRegistry");
        }
    }

    #[test]
    fn lower_codec_registry_with_contracts() {
        let src = r#"
            codec_registry ImageFormats {
                output: ImageOutput,

                codec Png {
                    magic: [0x89, 0x50],
                    decoder: decode_png,
                    contracts: {
                        ensures { result.width >= 1 }
                    }
                }
            }
        "#;
        let file = crate::parse_unwrap(src);
        if let Decl::CodecRegistry(cr) = &file.decls[0].node {
            assert_eq!(cr.codecs.len(), 1);
            assert!(
                !cr.codecs[0].contracts.is_empty(),
                "contracts should be non-empty"
            );
        } else {
            panic!("expected Decl::CodecRegistry");
        }
    }

    #[test]
    fn test_effect_row_with_variable() {
        let src = r#"
contract EffPoly {
    effects <io | E>
    fn map_with_effect(f: (Int) -> Int) -> List<Int>
}
"#;
        let sf = crate::parse_unwrap(src);
        assert_eq!(sf.decls.len(), 1);
        if let Decl::Contract(c) = &sf.decls[0].node {
            let eff_clause = c
                .clauses
                .iter()
                .find(|cl| cl.kind == ClauseKind::Effects)
                .expect("should have an effects clause");
            assert_eq!(
                eff_clause.effect_variables,
                vec!["E".to_string()],
                "should extract effect variable E from row"
            );
        } else {
            panic!("expected Decl::Contract");
        }
    }

    #[test]
    fn test_effect_row_multiple_variables() {
        let src = r#"
contract MultiEff {
    effects <io, net | E, F>
    fn poly_fn() -> Unit
}
"#;
        let sf = crate::parse_unwrap(src);
        if let Decl::Contract(c) = &sf.decls[0].node {
            let eff_clause = c
                .clauses
                .iter()
                .find(|cl| cl.kind == ClauseKind::Effects)
                .expect("should have an effects clause");
            assert_eq!(
                eff_clause.effect_variables,
                vec!["E".to_string(), "F".to_string()],
                "should extract both effect variables E and F"
            );
        } else {
            panic!("expected Decl::Contract");
        }
    }

    #[test]
    fn test_effect_row_no_variables() {
        let src = r#"
contract ConcreteEff {
    effects <io>
    fn concrete_fn() -> Unit
}
"#;
        let sf = crate::parse_unwrap(src);
        if let Decl::Contract(c) = &sf.decls[0].node {
            let eff_clause = c
                .clauses
                .iter()
                .find(|cl| cl.kind == ClauseKind::Effects)
                .expect("should have an effects clause");
            assert!(
                eff_clause.effect_variables.is_empty(),
                "effects without pipe should have no effect variables, got: {:?}",
                eff_clause.effect_variables
            );
        } else {
            panic!("expected Decl::Contract");
        }
    }

    #[test]
    fn lower_braced_clause_body_has_correct_original_spans() {
        // Regression for #335: expressions inside `requires { ... }` (and similar)
        // must have SpExpr spans that are byte offsets into the *original* source,
        // not compressed trivia-stripped coordinates pointing inside the keyword.
        let src = r#"
contract BadExpr {
    requires { 1 + true // inline comment after expr
    }
}
"#;
        let (sf, errors) = parse_and_lower(src);
        assert!(errors.is_empty(), "parse/lower errors: {errors:?}");
        if let Decl::Contract(c) = &sf.decls[0].node {
            assert!(!c.clauses.is_empty(), "expected at least one clause");
            let body = &c.clauses[0].body;
            // body span must refer to real content around the expression
            let body_text = &src[body.span.clone()];
            assert!(
                body_text.contains("1") && body_text.contains("true"),
                "braced body span {:?} should cover '1 + true' (or compacted), got {:?}",
                body.span,
                body_text
            );
            // Sub-expressions too (BinOp children)
            if let Expr::BinOp { lhs, rhs, .. } = &body.node {
                let lhs_text = &src[lhs.span.clone()];
                let rhs_text = &src[rhs.span.clone()];
                assert!(
                    lhs_text.contains('1') || lhs_text.trim() == "1",
                    "lhs span should point at literal 1, got span {:?} text {:?}",
                    lhs.span,
                    lhs_text
                );
                assert!(
                    rhs_text.contains("true"),
                    "rhs span should point at 'true', got span {:?} text {:?}",
                    rhs.span,
                    rhs_text
                );
            }
        } else {
            panic!("expected Contract decl");
        }
    }
}
