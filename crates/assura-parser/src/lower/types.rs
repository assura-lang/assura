//! Type-definition lowering: CST type/enum/field nodes → AST types.

use crate::ast::*;
use crate::cst;
use crate::syntax_kind::SyntaxKind;

use super::SyntaxNode;

pub(super) fn lower_type_def(n: &SyntaxNode) -> TypeDef {
    let name = super::first_ident(n);
    let type_params = super::lower_type_params(n);

    // Determine body type by looking at the structure
    let body = if super::has_token(n, SyntaxKind::EQUALS) {
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
                k if cst::is_trivia(k) => {}
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
            if cst::is_trivia(tok.kind()) {
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

pub(super) fn lower_field_def(n: &SyntaxNode) -> FieldDef {
    let is_pub = super::has_token(n, SyntaxKind::PUB_KW);

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
            // Keep COMMA: needed for `Map<K, V>` and tuple types `(Int, Bool)`.
            // Field separators live outside the FIELD_DEF CST node; stripping
            // commas here turned `(,)` into `()` (Unit) and broke generics.
            if t.kind() == SyntaxKind::SEMICOLON || cst::is_trivia(t.kind()) {
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

pub(super) fn lower_enum_def(n: &SyntaxNode) -> EnumDef {
    let name = super::first_ident(n);
    let type_params = super::lower_type_params(n);
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
    let name = super::first_ident(n);
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
                k if cst::is_trivia(k) => {}
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
