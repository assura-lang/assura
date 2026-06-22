//! Item-level grammar rules: contracts, types, enums, functions,
//! extern declarations, services, and generic blocks.

use crate::cst::Parser;
use crate::syntax_kind::SyntaxKind;

use super::clauses;
use super::params;

/// Parse a top-level declaration, wrapped in a Spanned node for AST.
pub(crate) fn decl(p: &mut Parser) {
    match p.current() {
        SyntaxKind::CONTRACT_KW => contract_decl(p),
        SyntaxKind::SERVICE_KW => service_decl(p),
        SyntaxKind::TYPE_KW => type_def(p),
        SyntaxKind::ENUM_KW => enum_def(p),
        SyntaxKind::EXTERN_KW => extern_decl(p),
        SyntaxKind::BIND_KW => bind_decl(p),
        SyntaxKind::CODEC_REGISTRY_KW => codec_registry_decl(p),
        SyntaxKind::FN_KW | SyntaxKind::AXIOM_KW | SyntaxKind::LEMMA_KW => fn_def(p),
        // Modifier-prefixed fn: pure/ghost/opaque + fn/axiom/lemma
        SyntaxKind::PURE_KW | SyntaxKind::OPAQUE_KW => {
            if is_fn_ahead(p) {
                fn_def(p);
            } else {
                generic_block(p);
            }
        }
        SyntaxKind::PROPHECY_KW => prophecy_decl(p),
        SyntaxKind::GHOST_KW => {
            if p.nth(1) == SyntaxKind::PROPHECY_KW {
                prophecy_decl(p);
            } else if is_fn_ahead(p) {
                fn_def(p);
            } else {
                generic_block(p);
            }
        }
        // #[attr] on items
        SyntaxKind::HASH => {
            if is_fn_ahead_past_attrs(p) {
                fn_def(p);
            } else {
                generic_block(p);
            }
        }
        // Keyword-as-block-name (spec, feature, incremental, etc.) or ident
        _ => generic_block(p),
    }
}

/// Look ahead past modifier keywords to see if fn/axiom/lemma follows.
fn is_fn_ahead(p: &mut Parser) -> bool {
    let mut lookahead = 0usize;
    loop {
        let k = p.nth(lookahead);
        if matches!(
            k,
            SyntaxKind::PURE_KW | SyntaxKind::GHOST_KW | SyntaxKind::OPAQUE_KW
        ) {
            lookahead += 1;
        } else {
            return matches!(
                k,
                SyntaxKind::FN_KW | SyntaxKind::AXIOM_KW | SyntaxKind::LEMMA_KW
            );
        }
    }
}

/// Look ahead past #[...] attributes and modifiers.
fn is_fn_ahead_past_attrs(p: &mut Parser) -> bool {
    let mut lookahead = 0usize;
    // Skip #[...] attribute groups
    while p.nth(lookahead) == SyntaxKind::HASH {
        lookahead += 1;
        if p.nth(lookahead) == SyntaxKind::L_BRACKET {
            lookahead += 1;
            let mut depth = 1;
            while depth > 0 && p.nth(lookahead) != SyntaxKind::ERROR_TOKEN {
                if p.nth(lookahead) == SyntaxKind::L_BRACKET {
                    depth += 1;
                } else if p.nth(lookahead) == SyntaxKind::R_BRACKET {
                    depth -= 1;
                }
                lookahead += 1;
            }
        }
    }
    // Skip modifiers
    while matches!(
        p.nth(lookahead),
        SyntaxKind::PURE_KW | SyntaxKind::GHOST_KW | SyntaxKind::OPAQUE_KW
    ) {
        lookahead += 1;
    }
    matches!(
        p.nth(lookahead),
        SyntaxKind::FN_KW | SyntaxKind::AXIOM_KW | SyntaxKind::LEMMA_KW
    )
}

/// contract Name<T> { clauses and embedded items }
fn contract_decl(p: &mut Parser) {
    let m = p.open();
    p.expect(SyntaxKind::CONTRACT_KW);
    p.expect(SyntaxKind::IDENT);
    params::type_params(p);

    p.expect(SyntaxKind::L_BRACE);
    p.bump_delim();
    while !p.eof() && !p.at(SyntaxKind::R_BRACE) {
        if clauses::at_clause_start(p) {
            clauses::clause(p);
        } else if p.at(SyntaxKind::TYPE_KW) {
            type_def(p);
        } else if p.at(SyntaxKind::ENUM_KW) {
            enum_def(p);
        } else if p.at(SyntaxKind::EXTERN_KW) {
            extern_decl(p);
        } else if p.at(SyntaxKind::BIND_KW) {
            bind_decl(p);
        } else if (matches!(
            p.current(),
            SyntaxKind::FN_KW
                | SyntaxKind::AXIOM_KW
                | SyntaxKind::LEMMA_KW
                | SyntaxKind::PURE_KW
                | SyntaxKind::GHOST_KW
                | SyntaxKind::OPAQUE_KW
        ) && is_fn_ahead(p))
            || (p.current() == SyntaxKind::HASH && is_fn_ahead_past_attrs(p))
        {
            fn_def(p);
        } else if p.at_keyword_or_ident() {
            // Generic blocks inside contracts (feature, incremental, etc.)
            generic_block(p);
        } else {
            p.err_and_bump("expected clause, type, fn, or closing brace");
        }
    }
    p.expect(SyntaxKind::R_BRACE);
    m.complete(p, SyntaxKind::CONTRACT_DECL);
}

/// type Name<T> = Alias | { fields } | = { refined }
fn type_def(p: &mut Parser) {
    let m = p.open();
    p.expect(SyntaxKind::TYPE_KW);
    p.expect(SyntaxKind::IDENT);
    params::type_params(p);

    if p.at(SyntaxKind::EQUALS) {
        p.bump(); // =
        if p.at(SyntaxKind::L_BRACE) {
            // Refined: = { ... }
            p.bump_delim();
            super::body_tokens_inner(p, &[]);
            p.expect(SyntaxKind::R_BRACE);
        } else {
            // Alias: = Type tokens until next decl
            type_alias_tokens(p);
        }
    } else if p.at(SyntaxKind::L_BRACE) {
        // Struct body: { fields, optional clauses }
        p.bump_delim();
        while !p.eof() && !p.at(SyntaxKind::R_BRACE) {
            let before = p.pos();
            if clauses::at_clause_start(p) {
                clauses::clause(p);
            } else {
                params::field_def(p);
            }
            if p.pos() == before {
                p.err_and_bump("expected field, clause, or `}`");
            }
        }
        p.expect(SyntaxKind::R_BRACE);
    }
    // else: empty type (just `type Foo`)

    p.eat(SyntaxKind::SEMICOLON);
    m.complete(p, SyntaxKind::TYPE_DEF);
}

/// Collect type alias tokens until a declaration-starting keyword or brace.
fn type_alias_tokens(p: &mut Parser) {
    while !p.eof() {
        let cur = p.current();
        if matches!(
            cur,
            SyntaxKind::SEMICOLON
                | SyntaxKind::L_BRACE
                | SyntaxKind::R_BRACE
                | SyntaxKind::CONTRACT_KW
                | SyntaxKind::TYPE_KW
                | SyntaxKind::ENUM_KW
                | SyntaxKind::EXTERN_KW
                | SyntaxKind::BIND_KW
                | SyntaxKind::FN_KW
                | SyntaxKind::SERVICE_KW
                | SyntaxKind::IMPORT_KW
                | SyntaxKind::MODULE_KW
                | SyntaxKind::PROJECT_KW
                | SyntaxKind::AXIOM_KW
                | SyntaxKind::LEMMA_KW
                | SyntaxKind::REQUIRES_KW
                | SyntaxKind::ENSURES_KW
                | SyntaxKind::EFFECTS_KW
                | SyntaxKind::INVARIANT_KW
                | SyntaxKind::MODIFIES_KW
                | SyntaxKind::INPUT_KW
                | SyntaxKind::OUTPUT_KW
                | SyntaxKind::RULE_KW
                | SyntaxKind::DATA_FLOW_KW
                | SyntaxKind::MUST_NOT_KW
        ) {
            break;
        }
        p.bump();
    }
}

/// enum Name<T> { Variant1(fields), Variant2, ... }
fn enum_def(p: &mut Parser) {
    let m = p.open();
    p.expect(SyntaxKind::ENUM_KW);
    p.expect(SyntaxKind::IDENT);
    params::type_params(p);

    if !p.at(SyntaxKind::L_BRACE) {
        // No opening brace -- error and complete the node as-is.
        p.error_at_current(format!("expected {:?}", SyntaxKind::L_BRACE));
        m.complete(p, SyntaxKind::ENUM_DEF);
        return;
    }
    p.bump(); // {
    while !p.eof() && !p.at(SyntaxKind::R_BRACE) {
        let before = p.pos();
        enum_variant(p);
        if p.pos() == before {
            // No progress -- skip the stuck token to avoid infinite loop.
            p.err_and_bump("expected enum variant or closing brace");
        }
    }
    p.expect(SyntaxKind::R_BRACE);
    m.complete(p, SyntaxKind::ENUM_DEF);
}

/// Variant[(fields)] [,]
fn enum_variant(p: &mut Parser) {
    let m = p.open();
    p.expect(SyntaxKind::IDENT);

    // Optional fields: (type1, type2)
    if p.at(SyntaxKind::L_PAREN) {
        p.bump(); // (
        super::body_tokens_inner(p, &[]);
        p.expect(SyntaxKind::R_PAREN);
    }

    p.eat(SyntaxKind::COMMA);
    m.complete(p, SyntaxKind::ENUM_VARIANT);
}

/// extern fn name(params) [-> RetType] [clauses] [;]
fn extern_decl(p: &mut Parser) {
    let m = p.open();
    p.expect(SyntaxKind::EXTERN_KW);
    p.expect(SyntaxKind::FN_KW);
    p.expect(SyntaxKind::IDENT);
    params::param_list(p);
    params::opt_return_type(p);

    // Clauses
    while !p.eof() && clauses::at_clause_start(p) {
        clauses::clause(p);
    }

    p.eat(SyntaxKind::SEMICOLON);
    m.complete(p, SyntaxKind::EXTERN_DECL);
}

/// bind "rust::path::to::fn" as name { input(...) output(...) clauses... }
fn bind_decl(p: &mut Parser) {
    let m = p.open();
    p.expect(SyntaxKind::BIND_KW);

    // Target path: string literal
    p.expect(SyntaxKind::STRING_LIT);

    // "as" Ident
    p.expect(SyntaxKind::AS_KW);
    p.expect(SyntaxKind::IDENT);

    // Body: { input(...) output(...) requires/ensures/effects }
    p.expect(SyntaxKind::L_BRACE);
    p.bump_delim();
    while !p.eof() && !p.at(SyntaxKind::R_BRACE) {
        if clauses::at_clause_start(p) {
            clauses::clause(p);
        } else {
            // Skip unknown tokens inside bind body
            p.bump();
        }
    }
    p.expect(SyntaxKind::R_BRACE);

    m.complete(p, SyntaxKind::BIND_DECL);
}

/// codec_registry Name { output: Type, codec Name { magic: [...], decoder: fn, contracts: { ... } } }
fn codec_registry_decl(p: &mut Parser) {
    let m = p.open();
    p.expect(SyntaxKind::CODEC_REGISTRY_KW);
    p.expect(SyntaxKind::IDENT); // registry name

    p.expect(SyntaxKind::L_BRACE);
    p.bump_delim();
    // output: Type,
    if p.at(SyntaxKind::OUTPUT_KW) {
        p.bump(); // output
        p.expect(SyntaxKind::COLON);
        // Consume type tokens until comma
        while !p.eof()
            && !p.at(SyntaxKind::COMMA)
            && !p.at(SyntaxKind::CODEC_KW)
            && !p.at(SyntaxKind::R_BRACE)
        {
            p.bump();
        }
        p.eat(SyntaxKind::COMMA);
    }

    // codec entries
    while !p.eof() && !p.at(SyntaxKind::R_BRACE) {
        if p.at(SyntaxKind::CODEC_KW) {
            codec_entry(p);
        } else {
            p.err_and_bump("expected `codec` or `}`");
        }
    }
    p.expect(SyntaxKind::R_BRACE);
    m.complete(p, SyntaxKind::CODEC_REGISTRY_DECL);
}

/// codec Name { magic: [...], decoder: fn [, contracts: { ... }] }
fn codec_entry(p: &mut Parser) {
    let m = p.open();
    p.expect(SyntaxKind::CODEC_KW);
    p.expect(SyntaxKind::IDENT); // codec name

    p.expect(SyntaxKind::L_BRACE);
    p.bump_delim();
    while !p.eof() && !p.at(SyntaxKind::R_BRACE) {
        let before = p.pos();
        if p.at(SyntaxKind::MAGIC_KW) {
            p.bump(); // magic
            p.expect(SyntaxKind::COLON);
            magic_pattern(p);
            p.eat(SyntaxKind::COMMA);
        } else if p.at_keyword_or_ident() && p.current_text() == "decoder" {
            p.bump(); // decoder
            p.expect(SyntaxKind::COLON);
            // Consume decoder fn name tokens until comma or brace
            while !p.eof() && !p.at(SyntaxKind::COMMA) && !p.at(SyntaxKind::R_BRACE) {
                if p.at_keyword_or_ident() && p.current_text() == "contracts" {
                    break;
                }
                p.bump();
            }
            p.eat(SyntaxKind::COMMA);
        } else if p.at_keyword_or_ident() && p.current_text() == "contracts" {
            p.bump(); // contracts
            p.expect(SyntaxKind::COLON);
            p.expect(SyntaxKind::L_BRACE);
            while !p.eof() && !p.at(SyntaxKind::R_BRACE) {
                if clauses::at_clause_start(p) {
                    clauses::clause(p);
                } else {
                    p.err_and_bump("expected clause or `}`");
                }
            }
            p.expect(SyntaxKind::R_BRACE);
        } else {
            // Skip unknown fields
            p.err_and_bump("expected `magic`, `decoder`, `contracts`, or `}`");
        }
        if p.pos() == before {
            p.err_and_bump("stuck in codec entry");
            break;
        }
    }
    p.expect(SyntaxKind::R_BRACE);
    m.complete(p, SyntaxKind::CODEC_ENTRY);
}

/// MagicPattern: [bytes..] | extension("ext", ...) | probe(fn_name)
fn magic_pattern(p: &mut Parser) {
    if p.at(SyntaxKind::L_BRACKET) {
        // BytePattern: [0x89, 0x50, ..., ..]
        p.bump(); // [
        while !p.eof() && !p.at(SyntaxKind::R_BRACKET) {
            p.bump(); // byte literal, comma, or ..
        }
        p.expect(SyntaxKind::R_BRACKET);
    } else if p.at_keyword_or_ident() && p.current_text() == "extension" {
        p.bump(); // extension
        p.expect(SyntaxKind::L_PAREN);
        while !p.eof() && !p.at(SyntaxKind::R_PAREN) {
            p.bump(); // string literals and commas
        }
        p.expect(SyntaxKind::R_PAREN);
    } else if p.at_keyword_or_ident() && p.current_text() == "probe" {
        p.bump(); // probe
        p.expect(SyntaxKind::L_PAREN);
        while !p.eof() && !p.at(SyntaxKind::R_PAREN) {
            p.bump(); // function name
        }
        p.expect(SyntaxKind::R_PAREN);
    } else {
        p.error_at_current(
            "expected byte pattern `[...]`, `extension(...)`, or `probe(...)`".into(),
        );
    }
}

/// `[ghost] prophecy <name> : <Type>`
fn prophecy_decl(p: &mut Parser) {
    let m = p.open();
    p.eat(SyntaxKind::GHOST_KW);
    p.expect(SyntaxKind::PROPHECY_KW);
    p.expect(SyntaxKind::IDENT); // name
    if p.at(SyntaxKind::COLON) {
        p.bump(); // ':'
        // Consume type tokens until next declaration or clause keyword
        while !p.eof() && !clauses::is_clause_stopper(p) {
            p.bump();
        }
    }
    m.complete(p, SyntaxKind::PROPHECY_DECL);
}

/// [#[attr]] [pure|ghost|opaque]* (fn|axiom|lemma) name<T>(params) [-> RetType] [= { body }] [clauses] [{ body }]
fn fn_def(p: &mut Parser) {
    let m = p.open();

    // Skip #[attr] groups
    while p.at(SyntaxKind::HASH) {
        let am = p.open();
        p.bump(); // #
        if p.at(SyntaxKind::L_BRACKET) {
            p.bump(); // [
            super::body_tokens_inner(p, &[]);
            p.expect(SyntaxKind::R_BRACKET);
        }
        am.complete(p, SyntaxKind::ATTR);
    }

    // Modifiers: pure, ghost, opaque
    while matches!(
        p.current(),
        SyntaxKind::PURE_KW | SyntaxKind::GHOST_KW | SyntaxKind::OPAQUE_KW
    ) {
        p.bump();
    }

    // fn | axiom | lemma
    if p.at_any(&[
        SyntaxKind::FN_KW,
        SyntaxKind::AXIOM_KW,
        SyntaxKind::LEMMA_KW,
    ]) {
        p.bump();
    } else {
        p.error_at_current("expected fn, axiom, or lemma".into());
    }

    // Name
    p.expect(SyntaxKind::IDENT);
    params::type_params(p);
    params::param_list(p);

    // Return type: -> Type or : Type
    if p.at(SyntaxKind::ARROW) || p.at(SyntaxKind::COLON) {
        params::fn_return_type(p);
    }

    // Optional = { body } (axiom definitions)
    if p.at(SyntaxKind::EQUALS) {
        p.bump();
        if p.at(SyntaxKind::L_BRACE) {
            p.bump_delim();
            super::body_tokens_inner(p, &[]);
            p.expect(SyntaxKind::R_BRACE);
        }
    }

    // Clauses
    while !p.eof() && clauses::at_clause_start(p) {
        clauses::clause(p);
    }

    // Optional trailing body { ... }
    if p.at(SyntaxKind::L_BRACE) {
        p.bump_delim();
        super::body_tokens_inner(p, &[]);
        p.expect(SyntaxKind::R_BRACE);
    }

    m.complete(p, SyntaxKind::FN_DEF);
}

/// service Name { items }
fn service_decl(p: &mut Parser) {
    let m = p.open();
    p.expect(SyntaxKind::SERVICE_KW);
    p.expect(SyntaxKind::IDENT);

    p.expect(SyntaxKind::L_BRACE);
    p.bump_delim();
    while !p.eof() && !p.at(SyntaxKind::R_BRACE) {
        let before = p.pos();
        service_item(p);
        if p.pos() == before {
            p.err_and_bump("expected service item or `}`");
        }
    }
    p.expect(SyntaxKind::R_BRACE);
    m.complete(p, SyntaxKind::SERVICE_DECL);
}

/// Items inside a service block.
fn service_item(p: &mut Parser) {
    match p.current() {
        SyntaxKind::TYPE_KW => type_def(p),
        SyntaxKind::ENUM_KW => enum_def(p),
        SyntaxKind::STATES_KW => {
            let m = p.open();
            p.bump(); // states
            p.expect(SyntaxKind::COLON);
            p.expect(SyntaxKind::IDENT);
            while p.at(SyntaxKind::ARROW) {
                p.bump(); // ->
                p.expect(SyntaxKind::IDENT);
            }
            m.complete(p, SyntaxKind::SERVICE_ITEM);
        }
        SyntaxKind::OPERATION_KW | SyntaxKind::QUERY_KW => {
            let m = p.open();
            p.bump(); // operation | query
            p.expect(SyntaxKind::IDENT);
            if p.at(SyntaxKind::L_BRACE) {
                p.bump(); // {
                while !p.eof() && !p.at(SyntaxKind::R_BRACE) {
                    if clauses::at_clause_start(p) {
                        clauses::clause(p);
                    } else {
                        p.err_and_bump("expected clause or closing brace");
                    }
                }
                p.expect(SyntaxKind::R_BRACE);
            } else {
                while !p.eof() && clauses::at_clause_start(p) {
                    clauses::clause(p);
                }
            }
            m.complete(p, SyntaxKind::SERVICE_ITEM);
        }
        SyntaxKind::INVARIANT_KW => {
            let m = p.open();
            p.bump();
            clauses::clause_body(p);
            m.complete(p, SyntaxKind::SERVICE_ITEM);
        }
        _ => {
            if p.at_keyword_or_ident() {
                let m = p.open();
                p.bump();
                clauses::clause_body(p);
                m.complete(p, SyntaxKind::SERVICE_ITEM);
            } else {
                p.err_and_bump("expected service item");
            }
        }
    }
}

/// Generic catch-all block: kind [name] [<T>] [: value | = value] [{ body }] [clauses]
pub(crate) fn generic_block(p: &mut Parser) {
    let m = p.open();

    // Skip #[attr] groups
    while p.at(SyntaxKind::HASH) {
        let am = p.open();
        p.bump(); // #
        if p.at(SyntaxKind::L_BRACKET) {
            p.bump(); // [
            super::body_tokens_inner(p, &[]);
            p.expect(SyntaxKind::R_BRACKET);
        }
        am.complete(p, SyntaxKind::ATTR);
    }

    // Kind name (keyword or ident)
    if p.at_keyword_or_ident() {
        p.bump();
    } else {
        p.error_at_current("expected block kind name".into());
    }

    // Optional second name
    if p.at_keyword_or_ident() && !clauses::at_clause_start(p) && !p.at(SyntaxKind::L_BRACE) {
        p.bump();
    }

    params::type_params(p);

    // Optional inline value: `: value` or `= value`
    if p.at(SyntaxKind::COLON) || p.at(SyntaxKind::EQUALS) {
        p.bump();
        // Collect value tokens until brace, clause keyword, or decl
        while !p.eof() {
            if clauses::is_clause_stopper(p)
                || p.current() == SyntaxKind::L_BRACE
                || p.current() == SyntaxKind::R_BRACE
            {
                break;
            }
            let cur = p.current();
            if cur == SyntaxKind::L_PAREN {
                p.bump_delim();
                super::body_tokens_inner(p, &[]);
                p.eat(SyntaxKind::R_PAREN);
            } else if cur == SyntaxKind::L_BRACKET {
                p.bump_delim();
                super::body_tokens_inner(p, &[]);
                p.eat(SyntaxKind::R_BRACKET);
            } else {
                p.bump();
            }
        }
    }

    // Block body or inline clauses
    if p.at(SyntaxKind::L_BRACE) {
        p.bump_delim();
        while !p.eof() && !p.at(SyntaxKind::R_BRACE) {
            if clauses::at_clause_start(p) {
                clauses::clause(p);
            } else if matches!(
                p.current(),
                SyntaxKind::FN_KW
                    | SyntaxKind::AXIOM_KW
                    | SyntaxKind::LEMMA_KW
                    | SyntaxKind::PURE_KW
                    | SyntaxKind::GHOST_KW
                    | SyntaxKind::OPAQUE_KW
                    | SyntaxKind::TYPE_KW
                    | SyntaxKind::ENUM_KW
                    | SyntaxKind::EXTERN_KW
                    | SyntaxKind::BIND_KW
                    | SyntaxKind::CONTRACT_KW
                    | SyntaxKind::SERVICE_KW
                    | SyntaxKind::HASH
                    | SyntaxKind::SPEC_KW
            ) {
                decl(p);
            } else {
                p.err_and_bump("expected clause, declaration, or closing brace");
            }
        }
        p.expect(SyntaxKind::R_BRACE);
    } else {
        // Inline clauses (no brace block)
        while !p.eof() && clauses::at_clause_start(p) {
            clauses::clause(p);
        }
    }

    m.complete(p, SyntaxKind::GENERIC_BLOCK);
}
