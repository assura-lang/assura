//! Expression lowering: CST expression nodes → AST `Expr` values.

use crate::ast::*;
use crate::cst;
use crate::syntax_kind::SyntaxKind;

use super::SyntaxNode;
use super::pattern::lower_pattern;

pub(super) fn is_expr_kind(k: SyntaxKind) -> bool {
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

pub(super) fn lower_expr(n: &SyntaxNode) -> SpExpr {
    match n.kind() {
        SyntaxKind::LITERAL_EXPR => super::spanned(lower_literal(n), n),
        SyntaxKind::IDENT_EXPR => {
            let text = super::collect_text(n).trim().to_string();
            super::spanned(Expr::Ident(text), n)
        }
        SyntaxKind::SELF_EXPR => super::spanned(Expr::Ident("self".into()), n),
        SyntaxKind::RESULT_EXPR => super::spanned(Expr::Ident("result".into()), n),
        SyntaxKind::FIELD_EXPR => lower_field_expr(n),
        SyntaxKind::METHOD_CALL_EXPR => lower_method_call(n),
        SyntaxKind::CALL_EXPR => lower_call_expr(n),
        SyntaxKind::INDEX_EXPR => lower_index_expr(n),
        SyntaxKind::BIN_EXPR => super::spanned(lower_bin_expr(n), n),
        SyntaxKind::UNARY_EXPR => super::spanned(lower_unary_expr(n), n),
        SyntaxKind::OLD_EXPR => super::spanned(lower_old_expr(n), n),
        SyntaxKind::FORALL_EXPR => super::spanned(lower_quantifier(n, true), n),
        SyntaxKind::EXISTS_EXPR => super::spanned(lower_quantifier(n, false), n),
        SyntaxKind::IF_EXPR => super::spanned(lower_if_expr(n), n),
        SyntaxKind::PAREN_EXPR => {
            let inner = n.children().find_map(|c| {
                if is_expr_kind(c.kind()) {
                    Some(lower_expr(&c))
                } else {
                    None
                }
            });
            inner.unwrap_or(super::spanned(Expr::Raw(vec![]), n))
        }
        SyntaxKind::TUPLE_EXPR => {
            let items = lower_expr_children(n);
            super::spanned(Expr::Tuple(items), n)
        }
        SyntaxKind::LIST_EXPR => {
            let items = lower_expr_children(n);
            super::spanned(Expr::List(items), n)
        }
        SyntaxKind::CAST_EXPR => super::spanned(lower_cast_expr(n), n),
        SyntaxKind::GHOST_EXPR => {
            let inner = n.children().find_map(|c| {
                if is_expr_kind(c.kind()) {
                    Some(super::lower_sp_expr(&c))
                } else {
                    None
                }
            });
            super::spanned(
                Expr::Ghost(Box::new(inner.unwrap_or(super::missing_expr()))),
                n,
            )
        }
        SyntaxKind::APPLY_EXPR => super::spanned(lower_apply_expr(n), n),
        SyntaxKind::LET_EXPR => super::spanned(lower_let_expr(n), n),
        SyntaxKind::MATCH_EXPR => super::spanned(lower_match_expr(n), n),
        _ => {
            // Fallback: collect tokens as raw
            super::spanned(Expr::Raw(super::collect_token_texts(n)), n)
        }
    }
}

fn lower_literal(n: &SyntaxNode) -> Expr {
    let Some(tok) = n
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| !cst::is_trivia(t.kind()))
    else {
        return Expr::Raw(super::collect_token_texts(n));
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
    let obj = super::lower_first_child_expr_or_missing(n);

    // Field name is the last IDENT or keyword token
    let field = n
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| t.kind() == SyntaxKind::IDENT || t.kind().is_keyword())
        .last()
        .map(|t| t.text().to_string())
        .unwrap_or_default();

    super::spanned(Expr::Field(Box::new(obj), field), n)
}

fn lower_method_call(n: &SyntaxNode) -> SpExpr {
    let receiver = super::lower_first_child_expr_or_missing(n);

    // Method name: IDENT or keyword token after DOT
    let method = n
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| t.kind() == SyntaxKind::IDENT || t.kind().is_keyword())
        .last()
        .map(|t| t.text().to_string())
        .unwrap_or_default();

    let args = super::find_child(n, SyntaxKind::ARG_LIST)
        .map(|al| lower_arg_list(&al))
        .unwrap_or_default();

    super::spanned(
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
    let func_name = super::first_ident_or_keyword(n);
    let mut args = super::find_child(n, SyntaxKind::ARG_LIST)
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

    super::spanned(
        Expr::Call {
            func: Box::new(super::spanned(Expr::Ident(func_name), n)),
            args,
        },
        n,
    )
}

pub(super) fn lower_expr_children(n: &SyntaxNode) -> Vec<SpExpr> {
    n.children()
        .filter(|c| is_expr_kind(c.kind()))
        .map(|c| super::lower_sp_expr(&c))
        .collect()
}

fn lower_arg_list(n: &SyntaxNode) -> Vec<SpExpr> {
    lower_expr_children(n)
}

fn lower_index_expr(n: &SyntaxNode) -> SpExpr {
    let mut exprs = lower_expr_children(n).into_iter();
    let base = exprs.next().unwrap_or(super::missing_expr());
    let index = exprs.next().unwrap_or(super::missing_expr());

    super::spanned(
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
            result = super::apply_binop_chain(result, chain);
            return result.node;
        };

        let rhs = rhs_node
            .map(|c| super::lower_sp_expr(&c))
            .unwrap_or(super::missing_expr());

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
                    .map(|c| super::lower_sp_expr(&c))
                    .unwrap_or(super::missing_expr());
                let result = super::apply_binop_chain(base, chain);
                return result.node;
            }
        }
    }
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
    let inner = super::lower_first_child_expr_or_missing(n);

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
    let inner = super::lower_first_child_expr_or_missing(n);
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
    let domain = exprs.next().unwrap_or(super::missing_expr());
    let body = exprs.next().unwrap_or(super::missing_expr());

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
    let cond = exprs.next().unwrap_or(super::missing_expr());
    let then_branch = exprs.next().unwrap_or(super::missing_expr());
    let else_branch = exprs.next().map(Box::new);

    Expr::If {
        cond: Box::new(cond),
        then_branch: Box::new(then_branch),
        else_branch,
    }
}

fn lower_cast_expr(n: &SyntaxNode) -> Expr {
    let inner = super::lower_first_child_expr_or_missing(n);

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
    let lemma_name = super::first_ident(n);
    let args: Vec<SpExpr> = super::find_child(n, SyntaxKind::ARG_LIST)
        .map(|al| lower_arg_list(&al))
        .unwrap_or_default();
    Expr::Apply { lemma_name, args }
}

fn lower_let_expr(n: &SyntaxNode) -> Expr {
    let name = super::first_ident(n);
    let mut exprs = lower_expr_children(n).into_iter();
    let value = exprs.next().unwrap_or(super::missing_expr());
    let body = exprs.next().unwrap_or(super::missing_expr());

    Expr::Let {
        name,
        value: Box::new(value),
        body: Box::new(body),
    }
}

fn lower_match_expr(n: &SyntaxNode) -> Expr {
    let scrutinee = super::lower_first_child_expr_or_missing(n);

    let arms = super::find_child(n, SyntaxKind::MATCH_ARM_LIST)
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
    // If this produces Wildcard when the source clearly had a literal/ident/constructor
    // pattern, the CST for the arm is missing the expected PAT child.
    // Common cause: missing p.bump_trivia() after eat(COMMA) in match_expr (or similar lists).
    // See grammar/expressions.rs and the trivia footgun docs in AGENTS.md.
    let pattern = n
        .children()
        .find_map(|c| lower_pattern(&c))
        .unwrap_or(Pattern::Wildcard);

    let exprs = lower_expr_children(n);
    let body = exprs.into_iter().last().unwrap_or(super::missing_expr());

    MatchArm { pattern, body }
}
