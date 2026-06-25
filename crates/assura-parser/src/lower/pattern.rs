//! Pattern lowering: CST pattern nodes → AST `Pattern` values.

use crate::ast::*;
use crate::syntax_kind::SyntaxKind;

use super::SyntaxNode;

pub(super) fn lower_pattern(n: &SyntaxNode) -> Option<Pattern> {
    match n.kind() {
        SyntaxKind::WILDCARD_PAT => Some(Pattern::Wildcard),
        SyntaxKind::IDENT_PAT => {
            let text = super::collect_text(n).trim().to_string();
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
            let name = super::first_ident(n);
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
