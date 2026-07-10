//! Declaration lowering: CST declaration nodes → AST `Decl` values.

use crate::ast::*;
use crate::cst;
use crate::syntax_kind::SyntaxKind;

use super::SyntaxNode;
use super::clause::{
    extract_params_from_clause_body, extract_return_type_from_clause_body, lower_clause,
    lower_clause_body,
};
use super::types::{lower_enum_def, lower_type_def};

// -----------------------------------------------------------------
// Contract
// -----------------------------------------------------------------

pub(super) fn lower_contract(n: &SyntaxNode) -> ContractDecl {
    let name = super::first_ident(n);
    let type_params = super::lower_type_params(n);
    let mut clauses: Vec<Clause> = n
        .children()
        .filter(|c| c.kind() == SyntaxKind::CLAUSE)
        .map(|c| lower_clause(&c))
        .collect();

    // Collect parameters from inline `fn` definitions inside the contract.
    // These params should be in scope for clause bodies (requires, ensures).
    let fn_params: Vec<Param> = n
        .children()
        .filter(|c| c.kind() == SyntaxKind::FN_DEF)
        .flat_map(|c| super::lower_param_list(&c))
        .collect();

    // If the contract has no `output(...)` but an inline `fn` declares a
    // return type, synthesize `output(result: T)` so SMT/codegen/IR see
    // Int (etc.) instead of Unit (dogfood: `fn id(x: Int) -> Int` alone).
    let has_output = clauses.iter().any(|c| c.kind == ClauseKind::Output);
    if !has_output {
        let first_fn = n.children().find(|c| c.kind() == SyntaxKind::FN_DEF);
        if let Some(fn_node) = first_fn {
            let ret_tokens = super::find_child(&fn_node, SyntaxKind::RETURN_TYPE)
                .map(|rt| super::collect_return_type_tokens(&rt))
                .unwrap_or_default();
            if !ret_tokens.is_empty() {
                let mut body_tokens = vec!["result".into(), ":".into()];
                body_tokens.extend(ret_tokens);
                clauses.push(Clause {
                    kind: ClauseKind::Output,
                    body: Spanned::no_span(Expr::Raw(body_tokens)),
                    effect_variables: vec![],
                });
            }
        }
    }

    ContractDecl {
        name,
        type_params,
        clauses,
        fn_params,
    }
}

// -----------------------------------------------------------------
// Extern
// -----------------------------------------------------------------

pub(super) fn lower_extern(n: &SyntaxNode) -> ExternDecl {
    let name = super::first_ident(n);
    let params = super::lower_param_list(n);
    let return_ty = super::find_child(n, SyntaxKind::RETURN_TYPE)
        .map(|rt| super::collect_return_type_tokens(&rt))
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
// ProphecyDecl
// -----------------------------------------------------------------

pub(super) fn lower_prophecy(n: &SyntaxNode) -> ProphecyDecl {
    // Skip ghost, prophecy keywords; find the name (first IDENT)
    let name = super::first_ident(n);
    // Collect type tokens after ':'
    let mut ty_tokens = Vec::new();
    let mut after_colon = false;
    for elem in n.children_with_tokens() {
        if let Some(tok) = elem.as_token() {
            if tok.kind() == SyntaxKind::COLON {
                after_colon = true;
                continue;
            }
            if after_colon && !cst::is_trivia(tok.kind()) {
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

// -----------------------------------------------------------------
// BindDecl
// -----------------------------------------------------------------

pub(super) fn lower_bind(n: &SyntaxNode) -> BindDecl {
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

    let name = super::first_ident(n);

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

// -----------------------------------------------------------------
// CodecRegistry
// -----------------------------------------------------------------

pub(super) fn lower_codec_registry(n: &SyntaxNode) -> CodecRegistryDecl {
    let name = super::first_ident(n);

    // Collect all non-whitespace tokens from the CODEC_REGISTRY_DECL node.
    // We'll walk them to extract output_type and codec entries.
    let tokens: Vec<(SyntaxKind, String)> = n
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| {
            let k = t.kind();
            !cst::is_trivia(k)
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
    let name = super::first_ident(n);

    let tokens: Vec<(SyntaxKind, String)> = n
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| {
            let k = t.kind();
            !cst::is_trivia(k)
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

// -----------------------------------------------------------------
// FnDef
// -----------------------------------------------------------------

pub(super) fn lower_fn_def(n: &SyntaxNode) -> FnDef {
    let name = super::first_ident(n);

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

    let params = super::lower_param_list(n);
    let return_ty = super::find_child(n, SyntaxKind::RETURN_TYPE)
        .map(|rt| super::collect_return_type_tokens(&rt))
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

pub(super) fn lower_service(n: &SyntaxNode) -> ServiceDecl {
    let name = super::first_ident(n);
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
            // Determine the sub-kind from tokens (skip trivia; trivia tokens are now present in CST)
            let first_tok = n
                .children_with_tokens()
                .filter_map(|el| el.into_token())
                .find(|t| !cst::is_trivia(t.kind()))
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
                    let name = super::first_ident(n);
                    let clauses: Vec<Clause> = n
                        .children()
                        .filter(|c| c.kind() == SyntaxKind::CLAUSE)
                        .map(|c| lower_clause(&c))
                        .collect();
                    Some(ServiceItem::Operation { name, clauses })
                }
                Some(SyntaxKind::QUERY_KW) => {
                    let name = super::first_ident(n);
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
                        .filter_map(|el| el.into_token())
                        .find(|t| !cst::is_trivia(t.kind()))
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

pub(super) fn lower_generic_block(n: &SyntaxNode) -> Decl {
    // First meaningful token is the kind
    let mut tokens_iter = n
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| !cst::is_trivia(t.kind()));

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
// Project / Module / Import
// -----------------------------------------------------------------

pub(super) fn lower_project(n: &SyntaxNode) -> ProjectDecl {
    let name = super::first_ident(n);
    let profile = super::find_child(n, SyntaxKind::PROFILE_LIST)
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

pub(super) fn lower_module(n: &SyntaxNode) -> ModuleDecl {
    let path = super::find_child(n, SyntaxKind::DOTTED_PATH)
        .map(|dp| lower_dotted_path(&dp))
        .unwrap_or_default();
    ModuleDecl { path }
}

pub(super) fn lower_dotted_path(n: &SyntaxNode) -> Vec<String> {
    n.children_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| t.kind() == SyntaxKind::IDENT)
        .map(|t| t.text().to_string())
        .collect()
}

pub(super) fn lower_import(n: &SyntaxNode) -> ImportDecl {
    let path = super::find_child(n, SyntaxKind::DOTTED_PATH)
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

    let items = super::find_child(n, SyntaxKind::IMPORT_ITEM_LIST)
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
