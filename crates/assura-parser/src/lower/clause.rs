//! Clause lowering: CST `CLAUSE` nodes → AST `Clause` values.

use crate::ast::*;
use crate::cst;
use crate::syntax_kind::SyntaxKind;

use super::{SyntaxNode, is_expr_kind};

pub(super) fn lower_clause(n: &SyntaxNode) -> Clause {
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
pub(super) fn lower_clause_body(n: &SyntaxNode) -> SpExpr {
    // Look for expression nodes (use descendants to find inner expr even
    // when wrapped in braces/parens/blocks for clause bodies like
    // `requires { 1 + true }`). This ensures the body SpExpr gets the
    // content's span (post 11.04).
    if let Some(expr_child) = n.descendants().find(|c| is_expr_kind(c.kind())) {
        return super::lower_sp_expr(&expr_child);
    }

    // Fall back to raw token collection.
    // Skip: the clause keyword, the single outer wrapper (`input(...)` /
    // `requires { ... }` / `input: { ... }`), and a leading `:` separator.
    // Keep *nested* delimiters so tuple types `t: (Int, Bool)` and calls
    // like `resolve(x)` survive (#899). Only the matching outer closer is
    // stripped (do not treat `)` inside a brace body as the outer closer).
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum OuterWrapper {
        None,
        Paren,
        Brace,
    }
    let mut saw_content = false;
    let mut skipped_kw = false;
    let mut outer = OuterWrapper::None;
    let mut depth: i32 = 0;
    let mut tokens: Vec<String> = Vec::new();
    for el in n.children_with_tokens() {
        match el {
            rowan::NodeOrToken::Token(t) => {
                let k = t.kind();
                if cst::is_trivia(k) {
                    continue;
                }
                // Skip the clause keyword (first significant token).
                if !skipped_kw {
                    skipped_kw = true;
                    continue;
                }
                // Skip leading colon (`input: { ... }`) before choosing wrapper.
                if k == SyntaxKind::COLON && !saw_content && outer == OuterWrapper::None {
                    continue;
                }
                // First delimiter after keyword/colon is the outer wrapper.
                if outer == OuterWrapper::None {
                    if k == SyntaxKind::L_PAREN {
                        outer = OuterWrapper::Paren;
                        depth = 1;
                        continue;
                    }
                    if k == SyntaxKind::L_BRACE {
                        outer = OuterWrapper::Brace;
                        depth = 1;
                        continue;
                    }
                    // Bare body (no wrapper).
                    outer = OuterWrapper::None;
                    saw_content = true;
                    tokens.push(t.text().to_string());
                    continue;
                }
                match (outer, k) {
                    (OuterWrapper::Paren, SyntaxKind::L_PAREN)
                    | (OuterWrapper::Brace, SyntaxKind::L_BRACE) => {
                        depth += 1;
                        saw_content = true;
                        tokens.push(t.text().to_string());
                    }
                    (OuterWrapper::Paren, SyntaxKind::R_PAREN)
                    | (OuterWrapper::Brace, SyntaxKind::R_BRACE) => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            continue; // matching outer closer only
                        }
                        saw_content = true;
                        tokens.push(t.text().to_string());
                    }
                    _ => {
                        // Nested opposite-kind delimiters always kept
                        // (e.g. `)` inside `requires { resolve(x) }`).
                        saw_content = true;
                        tokens.push(t.text().to_string());
                    }
                }
            }
            rowan::NodeOrToken::Node(child) => {
                saw_content = true;
                // Flatten nested CST nodes into individual tokens (preserve commas).
                tokens.extend(super::collect_token_texts(&child));
            }
        }
    }

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
    super::spanned(expr, n)
}

/// Extract parameters from a clause body like `a : Int , b : Int`.
pub(super) fn extract_params_from_clause_body(body: &Expr) -> Vec<Param> {
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
pub(super) fn extract_return_type_from_clause_body(body: &Expr) -> Vec<String> {
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
