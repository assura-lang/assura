//! CST -> AST lowering: convert a rowan `SyntaxNode` tree into the
//! existing `ast::*` types consumed by all downstream crates.
//!
//! This module walks the concrete syntax tree produced by the grammar
//! module and constructs the abstract syntax tree types defined in
//! `ast.rs`. The lowering is intentionally tolerant of missing nodes
//! (returns empty strings / empty vecs / defaults) so that it works
//! with error-recovered partial trees.
//!
//! # Sub-modules
//!
//! The lowering logic is split into focused sub-modules:
//!
//! - [`pattern`]: Pattern lowering (`WILDCARD_PAT`, `IDENT_PAT`, etc.)
//! - [`types`]: Type definition and enum lowering (`TypeDef`, `EnumDef`)
//! - [`clause`]: Clause lowering (`Clause`, `ClauseKind`)
//! - [`expr`]: Expression lowering (all `Expr` variants)
//! - [`decl`]: Declaration lowering (contracts, externs, binds, services, etc.)

mod clause;
mod decl;
mod expr;
mod pattern;
mod types;

use crate::ast::*;
use crate::cst;
use crate::syntax_kind::SyntaxKind;

use expr::is_expr_kind;

type SyntaxNode = crate::syntax_kind::SyntaxNode;

/// Lower a `SOURCE_FILE` node into an `ast::SourceFile`.
pub(crate) fn lower_source_file(root: &SyntaxNode) -> SourceFile {
    let mut project = None;
    let mut module = None;
    let mut imports = Vec::new();
    let mut decls = Vec::new();

    for child in root.children() {
        match child.kind() {
            SyntaxKind::PROJECT_DECL => project = Some(decl::lower_project(&child)),
            SyntaxKind::MODULE_DECL => module = Some(decl::lower_module(&child)),
            SyntaxKind::IMPORT_DECL => imports.push(decl::lower_import(&child)),
            SyntaxKind::CONTRACT_DECL => decls.push(spanned(
                Decl::Contract(decl::lower_contract(&child)),
                &child,
            )),
            SyntaxKind::SERVICE_DECL => {
                decls.push(spanned(Decl::Service(decl::lower_service(&child)), &child))
            }
            SyntaxKind::TYPE_DEF => decls.push(spanned(
                Decl::TypeDef(types::lower_type_def(&child)),
                &child,
            )),
            SyntaxKind::ENUM_DEF => decls.push(spanned(
                Decl::EnumDef(types::lower_enum_def(&child)),
                &child,
            )),
            SyntaxKind::EXTERN_DECL => {
                decls.push(spanned(Decl::Extern(decl::lower_extern(&child)), &child))
            }
            SyntaxKind::BIND_DECL => {
                decls.push(spanned(Decl::Bind(decl::lower_bind(&child)), &child))
            }
            SyntaxKind::PROPHECY_DECL => decls.push(spanned(
                Decl::Prophecy(decl::lower_prophecy(&child)),
                &child,
            )),
            SyntaxKind::CODEC_REGISTRY_DECL => decls.push(spanned(
                Decl::CodecRegistry(decl::lower_codec_registry(&child)),
                &child,
            )),
            SyntaxKind::FN_DEF => {
                decls.push(spanned(Decl::FnDef(decl::lower_fn_def(&child)), &child))
            }
            SyntaxKind::GENERIC_BLOCK => {
                decls.push(spanned(decl::lower_generic_block(&child), &child))
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
// Shared utilities (used by sub-modules via `super::`)
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
    expr::lower_expr(n)
}

/// Collect all token text from a node, optionally filtering by kind.
fn collect_text(n: &SyntaxNode) -> String {
    n.text().to_string()
}

/// Collect token texts as a Vec<String>, one per token.
fn collect_token_texts(n: &SyntaxNode) -> Vec<String> {
    n.descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| !cst::is_trivia(t.kind()))
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

/// Get the first identifier or keyword text from a node's direct token children.
fn first_ident_or_keyword(n: &SyntaxNode) -> String {
    first_token_text(n, |k, _| k == SyntaxKind::IDENT || k.is_keyword())
}

/// Find a child node of specific kind.
fn find_child(n: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxNode> {
    n.children().find(|c| c.kind() == kind)
}

/// Check if a node contains a specific token kind.
fn has_token(n: &SyntaxNode, kind: SyntaxKind) -> bool {
    n.children_with_tokens().any(|el| el.kind() == kind)
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

// -----------------------------------------------------------------
// Shared helpers for declarations
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
                if cst::is_trivia(tok.kind()) {
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
            if cst::is_trivia(t.kind()) {
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
#[path = "lower_tests.rs"]
mod tests;
