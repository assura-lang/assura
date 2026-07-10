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
    let mut ty: Vec<String> = n
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
            // Keep nested COMMA for `Map<K, V>` and `(Int, Bool)`. SEMICOLON
            // and trivia are never part of the type; trailing field separators
            // are stripped below (grammar eats `,`/`;` inside FIELD_DEF).
            if t.kind() == SyntaxKind::SEMICOLON || cst::is_trivia(t.kind()) {
                return false;
            }
            true
        })
        .map(|t| t.text().to_string())
        .collect();

    // Field terminators are inside FIELD_DEF (`p.eat(COMMA)` after the type).
    // Leaving a trailing `,` yields tokens like `["Int", ","]` → Named soup →
    // codegen `i64,,`. Nested commas are never last after a complete type
    // (`Map<K, V>` ends with `>`, `(Int,)` ends with `)`, `(,)` ends with `)`).
    while ty.last().is_some_and(|t| t == ",") {
        ty.pop();
    }

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
    // Payload types inside parens (if any), one token sequence per field.
    let fields = collect_paren_field_types(n);
    EnumVariant { name, fields }
}

/// Collect enum variant payload field types from `(T1, T2, …)`.
///
/// Splits only on top-level commas (depth 0 for `()`, `<>`, `[]`, `{}`) so
/// multi-token types like `(Int, Bool)`, `List<Int>`, and `Map<K, V>` stay
/// as a single field. Empty tuple `(,)` is one field with tokens
/// `["(", ",", ")"]` (invalid-empty marker after parse).
fn collect_paren_field_types(n: &SyntaxNode) -> Vec<Vec<String>> {
    let mut inside = false;
    let mut paren = 0i32;
    let mut angle = 0i32;
    let mut bracket = 0i32;
    let mut brace = 0i32;
    let mut fields: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();

    for el in n.children_with_tokens() {
        let Some(tok) = el.as_token() else {
            continue;
        };
        if cst::is_trivia(tok.kind()) {
            continue;
        }
        match tok.kind() {
            SyntaxKind::L_PAREN => {
                if !inside {
                    inside = true;
                } else {
                    paren += 1;
                    current.push("(".to_string());
                }
            }
            SyntaxKind::R_PAREN => {
                if !inside {
                    continue;
                }
                if paren > 0 {
                    paren -= 1;
                    current.push(")".to_string());
                } else {
                    // Outer closer: finish last field.
                    if !current.is_empty() {
                        fields.push(std::mem::take(&mut current));
                    }
                    break;
                }
            }
            SyntaxKind::L_ANGLE if inside => {
                angle += 1;
                current.push(tok.text().to_string());
            }
            SyntaxKind::R_ANGLE if inside => {
                angle = angle.saturating_sub(1);
                current.push(tok.text().to_string());
            }
            SyntaxKind::L_BRACKET if inside => {
                bracket += 1;
                current.push(tok.text().to_string());
            }
            SyntaxKind::R_BRACKET if inside => {
                bracket = bracket.saturating_sub(1);
                current.push(tok.text().to_string());
            }
            SyntaxKind::L_BRACE if inside => {
                brace += 1;
                current.push(tok.text().to_string());
            }
            SyntaxKind::R_BRACE if inside => {
                brace = brace.saturating_sub(1);
                current.push(tok.text().to_string());
            }
            SyntaxKind::COMMA
                if inside && paren == 0 && angle == 0 && bracket == 0 && brace == 0 =>
            {
                // Top-level field separator (including empty slots).
                fields.push(std::mem::take(&mut current));
            }
            _ if inside => {
                current.push(tok.text().to_string());
            }
            _ => {}
        }
    }
    fields
}
