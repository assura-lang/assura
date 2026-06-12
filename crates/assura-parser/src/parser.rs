use chumsky::prelude::*;
use crate::ast::*;
use crate::lexer::Token;

fn ident() -> impl Parser<Token, String, Error = Simple<Token>> + Clone {
    filter_map(|span, tok| match tok {
        Token::Ident(s) => Ok(s),
        _ => Err(Simple::expected_input_found(span, [], Some(tok))),
    })
}

/// Accept keyword tokens as identifiers (for extended syntax blocks, field names, etc.)
fn keyword_or_ident() -> impl Parser<Token, String, Error = Simple<Token>> + Clone {
    filter_map(|span, tok| match &tok {
        Token::Ident(s) => Ok(s.clone()),
        // Keywords that can appear as block kinds / field modifiers / identifiers
        Token::Ghost => Ok("ghost".into()),
        Token::Pure => Ok("pure".into()),
        Token::Axiom => Ok("axiom".into()),
        Token::Incremental => Ok("incremental".into()),
        Token::Liveness => Ok("liveness".into()),
        Token::Compliance => Ok("compliance".into()),
        Token::Concurrency => Ok("concurrency".into()),
        Token::Performance => Ok("performance".into()),
        Token::Privacy => Ok("privacy".into()),
        Token::Profile => Ok("profile".into()),
        Token::Protocol => Ok("protocol".into()),
        Token::Evolution => Ok("evolution".into()),
        Token::Ordering => Ok("ordering".into()),
        Token::Transaction => Ok("transaction".into()),
        Token::Fair => Ok("fair".into()),
        Token::Bind => Ok("bind".into()),
        Token::Resolve => Ok("resolve".into()),
        Token::Opaque => Ok("opaque".into()),
        Token::Lemma => Ok("lemma".into()),
        Token::Prophecy => Ok("prophecy".into()),
        Token::Idempotent => Ok("idempotent".into()),
        Token::Retention => Ok("retention".into()),
        Token::Input => Ok("input".into()),
        Token::Output => Ok("output".into()),
        Token::States => Ok("states".into()),
        Token::Rule => Ok("rule".into()),
        Token::Where => Ok("where".into()),
        Token::In => Ok("in".into()),
        Token::Is => Ok("is".into()),
        Token::Self_ => Ok("self".into()),
        Token::Result_ => Ok("result".into()),
        _ => Err(Simple::expected_input_found(span, [], Some(tok))),
    })
}

fn dotted_path() -> impl Parser<Token, Vec<String>, Error = Simple<Token>> + Clone {
    ident().separated_by(just(Token::Dot)).at_least(1)
}

fn type_params() -> impl Parser<Token, Vec<String>, Error = Simple<Token>> + Clone {
    // Support bounded params: <T: Trait, U: Bound>  -- keep just the names
    let bounded_param = ident()
        .then(just(Token::Colon).ignore_then(
            filter(|t: &Token| !matches!(t, Token::Comma | Token::RAngle))
                .repeated()
                .at_least(1),
        ).or_not())
        .map(|(name, _bound)| name);

    bounded_param
        .separated_by(just(Token::Comma))
        .delimited_by(just(Token::LAngle), just(Token::RAngle))
        .or_not()
        .map(|o| o.unwrap_or_default())
}

fn tok_to_str(t: &Token) -> String {
    match t {
        Token::Ident(s) | Token::Int(s) | Token::Float(s) | Token::String(s) => s.clone(),
        Token::True => "true".into(),
        Token::False => "false".into(),
        Token::LBrace => "{".into(),
        Token::RBrace => "}".into(),
        Token::LParen => "(".into(),
        Token::RParen => ")".into(),
        Token::LBracket => "[".into(),
        Token::RBracket => "]".into(),
        Token::LAngle => "<".into(),
        Token::RAngle => ">".into(),
        Token::Comma => ",".into(),
        Token::Colon => ":".into(),
        Token::Semicolon => ";".into(),
        Token::Dot => ".".into(),
        Token::Pipe => "|".into(),
        Token::Arrow => "->".into(),
        Token::Eq => "==".into(),
        Token::Neq => "!=".into(),
        Token::Lte => "<=".into(),
        Token::Gte => ">=".into(),
        Token::Plus => "+".into(),
        Token::Minus => "-".into(),
        Token::Star => "*".into(),
        Token::Slash => "/".into(),
        Token::Percent => "%".into(),
        Token::Hash => "#".into(),
        Token::At => "@".into(),
        Token::Equals => "=".into(),
        Token::AndAnd => "&&".into(),
        Token::OrOr => "||".into(),
        Token::Bang => "!".into(),
        Token::Question => "?".into(),
        Token::Concat => "++".into(),
        Token::FatArrow => "=>".into(),
        Token::Amp => "&".into(),
        Token::AmpMut => "&mut".into(),
        Token::DotDot => "..".into(),
        Token::Caret => "^".into(),
        _ => format!("{t:?}"),
    }
}

/// Collect all tokens until we hit a stopper, handling balanced groups.
fn body_tokens(
    stoppers: &'static [Token],
) -> impl Parser<Token, Vec<String>, Error = Simple<Token>> + Clone {
    recursive(|body| {
        let balanced_braces = just(Token::LBrace)
            .then(body.clone())
            .then(just(Token::RBrace))
            .map(|((_, mut inner), _)| {
                let mut v = vec!["{".to_string()];
                v.append(&mut inner);
                v.push("}".to_string());
                v
            });
        let balanced_parens = just(Token::LParen)
            .then(body.clone())
            .then(just(Token::RParen))
            .map(|((_, mut inner), _)| {
                let mut v = vec!["(".to_string()];
                v.append(&mut inner);
                v.push(")".to_string());
                v
            });
        let balanced_brackets = just(Token::LBracket)
            .then(body.clone())
            .then(just(Token::RBracket))
            .map(|((_, mut inner), _)| {
                let mut v = vec!["[".to_string()];
                v.append(&mut inner);
                v.push("]".to_string());
                v
            });
        let single = filter(move |t: &Token| {
            !stoppers.contains(t)
                && !matches!(
                    t,
                    Token::RBrace | Token::RParen | Token::RBracket
                )
        })
        .map(|t| vec![tok_to_str(&t)]);

        choice((balanced_braces, balanced_parens, balanced_brackets, single))
            .repeated()
            .flatten()
    })
}

// Clause stoppers
const CLAUSE_STOPS: &[Token] = &[];

fn clause_kind() -> impl Parser<Token, ClauseKind, Error = Simple<Token>> + Clone {
    choice((
        just(Token::Requires).to(ClauseKind::Requires),
        just(Token::Ensures).to(ClauseKind::Ensures),
        just(Token::Effects).to(ClauseKind::Effects),
        just(Token::Invariant).to(ClauseKind::Invariant),
        just(Token::Modifies).to(ClauseKind::Modifies),
        just(Token::Input).to(ClauseKind::Input),
        just(Token::Output).to(ClauseKind::Output),
        just(Token::Rule).to(ClauseKind::Rule),
        just(Token::DataFlow).to(ClauseKind::DataFlow),
        just(Token::MustNot).to(ClauseKind::MustNot),
        filter_map(|span, tok| match &tok {
            Token::Ident(s)
                if matches!(
                    s.as_str(),
                    "ghost" | "step" | "resume" | "assume" | "prove"
                        | "spec" | "validate" | "define" | "property"
                        | "errors" | "constant_time" | "taint"
                        | "verify" | "example" | "strategy"
                        | "must_be" | "promise" | "bound"
                        | "verify_against"
                        | "reads" | "writes"
                ) =>
            {
                Ok(ClauseKind::Other(s.clone()))
            }
            _ => Err(Simple::expected_input_found(span, [], Some(tok))),
        }),
    ))
}

fn clause_body() -> impl Parser<Token, Vec<String>, Error = Simple<Token>> + Clone {
    let braced = just(Token::Colon)
        .or_not()
        .ignore_then(body_tokens(CLAUSE_STOPS).delimited_by(just(Token::LBrace), just(Token::RBrace)));

    let parened = just(Token::Colon)
        .or_not()
        .ignore_then(body_tokens(CLAUSE_STOPS).delimited_by(just(Token::LParen), just(Token::RParen)));

    // Inline: colon then tokens until next clause keyword or }
    let inline = just(Token::Colon).ignore_then(
        filter(move |t: &Token| {
            // Stop at clause keywords and block delimiters
            !matches!(
                t,
                Token::Requires
                    | Token::Ensures
                    | Token::Effects
                    | Token::Invariant
                    | Token::Modifies
                    | Token::Input
                    | Token::Output
                    | Token::Rule
                    | Token::DataFlow
                    | Token::MustNot
                    | Token::LBrace
                    | Token::RBrace
            )
            // Stop at declaration-starting keywords
            && !matches!(
                t,
                Token::Contract | Token::Type | Token::Enum
                    | Token::Extern | Token::Fn | Token::Service
                    | Token::Import | Token::Module | Token::Project
                    | Token::Axiom | Token::Lemma
            )
            // Stop at ident-based clause/decl keywords
            && !matches!(t, Token::Ident(s) if matches!(s.as_str(),
                    "ghost" | "step" | "resume" | "assume" | "prove"
                        | "spec" | "validate" | "define" | "property"
                        | "errors" | "constant_time" | "taint"
                        | "verify" | "example" | "strategy"
                        | "must_be" | "promise" | "bound"
                        | "verify_against"
                        | "reads" | "writes"
                        | "operation" | "query" | "states"))
        })
        .map(|t| tok_to_str(&t))
        .repeated(),
    );

    // Bare: no colon, just space-separated tokens until next clause/decl keyword
    let bare = filter(move |t: &Token| {
        !matches!(
            t,
            Token::Requires | Token::Ensures | Token::Effects
                | Token::Invariant | Token::Modifies | Token::Input
                | Token::Output | Token::Rule | Token::DataFlow
                | Token::MustNot | Token::RBrace | Token::LBrace
                | Token::Contract | Token::Type | Token::Enum
                | Token::Extern | Token::Fn | Token::Service
                | Token::Import | Token::Module | Token::Project
                | Token::Ghost | Token::Pure | Token::Opaque
        ) && !matches!(t, Token::Ident(s) if matches!(s.as_str(),
                "ghost" | "step" | "resume" | "assume" | "prove"
                    | "spec" | "validate" | "define" | "property"
                    | "errors" | "constant_time" | "taint"
                    | "verify" | "example" | "strategy"
                    | "must_be" | "promise" | "bound"
                    | "operation" | "query" | "states"))
    })
    .map(|t| tok_to_str(&t))
    .repeated()
    .at_least(1);

    choice((braced, parened, inline, bare))
}

fn clause() -> impl Parser<Token, Clause, Error = Simple<Token>> + Clone {
    clause_kind()
        .then(clause_body())
        .map(|(kind, tokens)| Clause { kind, tokens })
}

// --- Contract ---

fn contract_decl() -> impl Parser<Token, ContractDecl, Error = Simple<Token>> + Clone {
    // Contract body can contain clauses, type defs, fn defs, etc.
    let contract_item = choice::<_, Simple<Token>>((
        clause().map(Some),
        // Skip embedded type/fn/enum/extern declarations inside contracts
        type_def().map(|_| None),
        fn_def().map(|_| None),
        enum_def().map(|_| None),
        extern_decl().map(|_| None),
        // Skip generic blocks (feature, incremental, etc.) inside contracts
        generic_block().map(|_| None),
    ));

    just(Token::Contract)
        .ignore_then(ident())
        .then(type_params())
        .then(contract_item.repeated().delimited_by(just(Token::LBrace), just(Token::RBrace)))
        .map(|((name, type_params), items)| ContractDecl {
            name,
            type_params,
            clauses: items.into_iter().flatten().collect(),
        })
}

// --- Type ---

fn field_def() -> impl Parser<Token, FieldDef, Error = Simple<Token>> + Clone {
    let vis = just(Token::Pub).or_not().map(|o| o.is_some());

    // Skip optional modifiers like `ghost var`
    let modifiers = choice((
        just(Token::Ghost), just(Token::Pure), just(Token::Opaque),
    )).repeated();

    // Skip optional `var` after ghost
    let var_kw = filter_map(|span, tok| match &tok {
        Token::Ident(s) if s == "var" => Ok(()),
        _ => Err(Simple::expected_input_found(span, [], Some(tok))),
    }).or_not();

    // Type tokens: collect everything except field terminators, but handle
    // balanced braces (for refinement types like {v: Nat | v == MaxLen})
    // and balanced parens/brackets.
    let field_type = recursive(|ty_body| {
        let balanced_braces = just(Token::LBrace)
            .then(ty_body.clone())
            .then(just(Token::RBrace))
            .map(|((_, mut inner), _)| {
                let mut v = vec!["{".to_string()];
                v.append(&mut inner);
                v.push("}".to_string());
                v
            });
        let balanced_parens = just(Token::LParen)
            .then(ty_body.clone())
            .then(just(Token::RParen))
            .map(|((_, mut inner), _)| {
                let mut v = vec!["(".to_string()];
                v.append(&mut inner);
                v.push(")".to_string());
                v
            });
        let balanced_brackets = just(Token::LBracket)
            .then(ty_body.clone())
            .then(just(Token::RBracket))
            .map(|((_, mut inner), _)| {
                let mut v = vec!["[".to_string()];
                v.append(&mut inner);
                v.push("]".to_string());
                v
            });
        let single = filter(|t: &Token| {
            !matches!(t, Token::Semicolon | Token::Comma
                | Token::RBrace | Token::RParen | Token::RBracket)
        }).map(|t| vec![tok_to_str(&t)]);

        choice((balanced_braces, balanced_parens, balanced_brackets, single))
            .repeated()
            .flatten()
    });

    vis.then(modifiers)
        .then(var_kw)
        .then(ident())
        .then_ignore(just(Token::Colon))
        .then(field_type)
        .then_ignore(just(Token::Semicolon).or(just(Token::Comma)).or_not())
        .map(|((((_is_pub, _mods), _var), name), ty)| FieldDef { name, ty, is_pub: _is_pub })
}

fn type_def() -> impl Parser<Token, TypeDef, Error = Simple<Token>> + Clone {
    just(Token::Type)
        .ignore_then(ident())
        .then(type_params())
        .then(choice((
            // Refined: = { ... }
            just(Token::Equals)
                .ignore_then(body_tokens(CLAUSE_STOPS).delimited_by(just(Token::LBrace), just(Token::RBrace)))
                .map(TypeBody::Refined),
            // Alias: = SomeType (stop at decl keywords and braces)
            just(Token::Equals)
                .ignore_then(
                    filter(|t: &Token| !matches!(t, Token::Semicolon | Token::LBrace | Token::RBrace
                        | Token::Contract | Token::Type | Token::Enum
                        | Token::Extern | Token::Fn | Token::Service
                        | Token::Import | Token::Module | Token::Project
                        | Token::Axiom | Token::Lemma
                        | Token::Requires | Token::Ensures | Token::Effects
                        | Token::Invariant | Token::Modifies | Token::Input
                        | Token::Output | Token::Rule | Token::DataFlow | Token::MustNot
                    ))
                        .map(|t| tok_to_str(&t))
                        .repeated()
                        .at_least(1),
                )
                .map(TypeBody::Alias),
            // Struct: { fields + optional invariant/clause blocks }
            choice::<_, Simple<Token>>((
                field_def().map(Some),
                // Skip invariant/clause blocks inside struct bodies
                clause().map(|_| None),
            ))
            .repeated()
            .delimited_by(just(Token::LBrace), just(Token::RBrace))
            .map(|items| TypeBody::Struct(items.into_iter().flatten().collect())),
            empty().to(TypeBody::Empty),
        )))
        .then_ignore(just(Token::Semicolon).or_not())
        .map(|((name, type_params), body)| TypeDef {
            name,
            type_params,
            body,
        })
}

// --- Enum ---

fn enum_variant() -> impl Parser<Token, EnumVariant, Error = Simple<Token>> + Clone {
    ident()
        .then(
            body_tokens(CLAUSE_STOPS)
                .delimited_by(just(Token::LParen), just(Token::RParen))
                .or_not(),
        )
        .then_ignore(just(Token::Comma).or_not())
        .map(|(name, fields)| EnumVariant {
            name,
            fields: fields.unwrap_or_default(),
        })
}

fn enum_def() -> impl Parser<Token, EnumDef, Error = Simple<Token>> + Clone {
    just(Token::Enum)
        .ignore_then(ident())
        .then(type_params())
        .then(enum_variant().repeated().delimited_by(just(Token::LBrace), just(Token::RBrace)))
        .map(|((name, type_params), variants)| EnumDef {
            name,
            type_params,
            variants,
        })
}

// --- Params ---

fn param() -> impl Parser<Token, Param, Error = Simple<Token>> + Clone {
    // Skip #[...] attributes
    let attr = just(Token::Hash)
        .then(body_tokens(CLAUSE_STOPS).delimited_by(just(Token::LBracket), just(Token::RBracket)))
        .repeated();

    // Type tokens with balanced delimiters (for refinement types like {v: Nat | v == X})
    // and #[attr] annotations
    let param_type = recursive(|ty_body| {
        let balanced_braces = just(Token::LBrace)
            .then(ty_body.clone())
            .then(just(Token::RBrace))
            .map(|((_, mut inner), _)| {
                let mut v = vec!["{".to_string()];
                v.append(&mut inner);
                v.push("}".to_string());
                v
            });
        let balanced_parens = just(Token::LParen)
            .then(ty_body.clone())
            .then(just(Token::RParen))
            .map(|((_, mut inner), _)| {
                let mut v = vec!["(".to_string()];
                v.append(&mut inner);
                v.push(")".to_string());
                v
            });
        let balanced_angles = just(Token::LAngle)
            .then(ty_body.clone())
            .then(just(Token::RAngle))
            .map(|((_, mut inner), _)| {
                let mut v = vec!["<".to_string()];
                v.append(&mut inner);
                v.push(">".to_string());
                v
            });
        let balanced_brackets = just(Token::LBracket)
            .then(ty_body.clone())
            .then(just(Token::RBracket))
            .map(|((_, mut inner), _)| {
                let mut v = vec!["[".to_string()];
                v.append(&mut inner);
                v.push("]".to_string());
                v
            });
        let single = filter(|t: &Token| {
            !matches!(t, Token::Comma | Token::RParen
                | Token::RBrace | Token::RBracket | Token::RAngle)
        }).map(|t| vec![tok_to_str(&t)]);

        choice((balanced_braces, balanced_parens, balanced_angles, balanced_brackets, single))
            .repeated()
            .flatten()
    });

    attr.ignore_then(keyword_or_ident())
        .then_ignore(just(Token::Colon))
        .then(param_type)
        .map(|(name, ty)| Param { name, ty })
}

fn param_list() -> impl Parser<Token, Vec<Param>, Error = Simple<Token>> + Clone {
    param()
        .separated_by(just(Token::Comma))
        .allow_trailing()
        .delimited_by(just(Token::LParen), just(Token::RParen))
}

fn return_type() -> impl Parser<Token, Vec<String>, Error = Simple<Token>> + Clone {
    just(Token::Arrow).ignore_then(
        filter(|t: &Token| {
            !matches!(
                t,
                Token::LBrace | Token::Requires | Token::Ensures
                    | Token::Effects | Token::Modifies
                    | Token::Invariant | Token::Input | Token::Output
                    | Token::Rule | Token::DataFlow | Token::MustNot
            )
            && !matches!(t, Token::Ident(s) if matches!(s.as_str(),
                    "must_be" | "promise" | "bound"))
        })
        .map(|t| tok_to_str(&t))
        .repeated()
        .at_least(1),
    )
}

// --- Extern ---

fn extern_decl() -> impl Parser<Token, ExternDecl, Error = Simple<Token>> + Clone {
    just(Token::Extern)
        .ignore_then(just(Token::Fn))
        .ignore_then(ident())
        .then(param_list())
        .then(return_type().or_not())
        .then(clause().repeated())
        .then_ignore(just(Token::Semicolon).or_not())
        .map(|(((name, params), ret), clauses)| ExternDecl {
            name,
            params,
            return_ty: ret.unwrap_or_default(),
            clauses,
        })
}

// --- Fn ---

fn fn_def() -> impl Parser<Token, FnDef, Error = Simple<Token>> + Clone {
    let attr = just(Token::Hash)
        .then(body_tokens(CLAUSE_STOPS).delimited_by(just(Token::LBracket), just(Token::RBracket)))
        .repeated();

    // Optional modifiers: pure, ghost, opaque
    let modifiers = choice((
        just(Token::Pure), just(Token::Ghost), just(Token::Opaque),
    )).repeated();

    // fn, axiom, lemma all have function-like syntax
    let fn_keyword = choice((
        just(Token::Fn),
        just(Token::Axiom),
        just(Token::Lemma),
    ));

    // Return type: `-> Type` or `: Type` (axioms use colon-style)
    // Return type: simple tokens only (no balanced braces at top level,
    // since { ... } after return type is the fn body, not a refinement type).
    // For `-> {v: Nat | v <= X}` refinement returns, we need balanced braces
    // but ONLY as the first token group after the arrow.
    let ret_type_tokens = {
        // First element can be a refinement type `{...}`
        let first_braced = just(Token::LBrace)
            .then(body_tokens(CLAUSE_STOPS))
            .then(just(Token::RBrace))
            .map(|((_, mut inner), _)| {
                let mut v = vec!["{".to_string()];
                v.append(&mut inner);
                v.push("}".to_string());
                v
            });
        let rest = filter(|t: &Token| {
            !matches!(
                t,
                Token::LBrace | Token::RBrace | Token::Requires | Token::Ensures
                    | Token::Effects | Token::Modifies | Token::Equals
                    | Token::Invariant | Token::Input | Token::Output
                    | Token::Rule | Token::DataFlow | Token::MustNot
            )
            && !matches!(t, Token::Ident(s) if matches!(s.as_str(),
                    "must_be" | "promise" | "bound"))
        }).map(|t| vec![tok_to_str(&t)]);

        choice((first_braced, rest))
            .repeated()
            .at_least(1)
            .flatten()
    };

    let ret_arrow = just(Token::Arrow).ignore_then(ret_type_tokens.clone());
    let ret_colon = just(Token::Colon).ignore_then(ret_type_tokens);

    // Optional `= { body }` for axiom definitions
    let eq_body = just(Token::Equals)
        .ignore_then(
            body_tokens(CLAUSE_STOPS)
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .or_not();

    attr.ignore_then(modifiers)
        .ignore_then(fn_keyword)
        .ignore_then(ident())
        .then(type_params())
        .then(param_list())
        .then(choice((ret_arrow, ret_colon)).or_not())
        .then(eq_body)
        .then(clause().repeated())
        .then(
            body_tokens(CLAUSE_STOPS)
                .delimited_by(just(Token::LBrace), just(Token::RBrace))
                .or_not(),
        )
        .map(|((((((name, _tps), params), ret), _eq_body), clauses), _body)| FnDef {
            name,
            params,
            return_ty: ret.unwrap_or_default(),
            clauses,
        })
}

// --- Service ---

fn service_item() -> impl Parser<Token, ServiceItem, Error = Simple<Token>> + Clone {
    choice((
        type_def().map(ServiceItem::TypeDef),
        enum_def().map(ServiceItem::EnumDef),
        just(Token::States)
            .ignore_then(just(Token::Colon))
            .ignore_then(ident().separated_by(just(Token::Arrow)).at_least(1))
            .map(ServiceItem::States),
        just(Token::Operation)
            .ignore_then(ident())
            .then(clause().repeated().delimited_by(just(Token::LBrace), just(Token::RBrace)))
            .map(|(name, clauses)| ServiceItem::Operation { name, clauses }),
        just(Token::Query)
            .ignore_then(ident())
            .then(clause().repeated().delimited_by(just(Token::LBrace), just(Token::RBrace)))
            .map(|(name, clauses)| ServiceItem::Query { name, clauses }),
        just(Token::Invariant)
            .ignore_then(clause_body())
            .map(ServiceItem::Invariant),
        ident()
            .then(clause_body())
            .map(|(kind, body)| ServiceItem::Other { kind, body }),
    ))
}

fn service_decl() -> impl Parser<Token, ServiceDecl, Error = Simple<Token>> + Clone {
    just(Token::Service)
        .ignore_then(ident())
        .then(service_item().repeated().delimited_by(just(Token::LBrace), just(Token::RBrace)))
        .map(|(name, items)| ServiceDecl { name, items })
}

// --- Generic block (incremental, liveness, feature, etc.) ---

fn generic_block() -> impl Parser<Token, Decl, Error = Simple<Token>> + Clone {
    let attr = just(Token::Hash)
        .then(body_tokens(CLAUSE_STOPS).delimited_by(just(Token::LBracket), just(Token::RBracket)))
        .repeated();

    // Inline value: `: Type = value` or `= value` (for feature / feature_max)
    // Value part: `: Type = expr` or `= expr` with balanced delimiters
    let value_tokens = recursive(|val_body| {
        let balanced_braces = just(Token::LBrace)
            .then(val_body.clone())
            .then(just(Token::RBrace))
            .map(|((_, mut inner), _)| {
                let mut v = vec!["{".to_string()];
                v.append(&mut inner);
                v.push("}".to_string());
                v
            });
        let balanced_parens = just(Token::LParen)
            .then(val_body.clone())
            .then(just(Token::RParen))
            .map(|((_, mut inner), _)| {
                let mut v = vec!["(".to_string()];
                v.append(&mut inner);
                v.push(")".to_string());
                v
            });
        let balanced_brackets = just(Token::LBracket)
            .then(val_body.clone())
            .then(just(Token::RBracket))
            .map(|((_, mut inner), _)| {
                let mut v = vec!["[".to_string()];
                v.append(&mut inner);
                v.push("]".to_string());
                v
            });
        let single = filter(|t: &Token| {
            !matches!(t, Token::RBrace | Token::RParen | Token::RBracket
                // Stop at clause keywords
                | Token::Requires | Token::Ensures | Token::Effects
                | Token::Invariant | Token::Modifies | Token::Input
                | Token::Output | Token::Rule | Token::DataFlow | Token::MustNot
                // Stop at declaration keywords
                | Token::Contract | Token::Type | Token::Enum
                | Token::Extern | Token::Fn | Token::Service
                | Token::Import | Token::Module | Token::Project
                | Token::Axiom | Token::Lemma
                | Token::Ghost | Token::Pure | Token::Opaque
            ) && !matches!(t, Token::Ident(s) if matches!(s.as_str(),
                    "must_be" | "promise" | "bound" | "define" | "property"
                    | "verify_against" | "constant_time"))
        }).map(|t| vec![tok_to_str(&t)]);

        choice((balanced_braces, balanced_parens, balanced_brackets, single))
            .repeated()
            .flatten()
    });

    let inline_value = choice((
        just(Token::Colon).ignore_then(value_tokens.clone()),
        just(Token::Equals).ignore_then(value_tokens),
    )).or_not();

    // Block body items: clauses, embedded fns, types, enums, or nested blocks
    let block_item = choice::<_, Simple<Token>>((
        clause().map(Some),
        fn_def().map(|_| None),
        type_def().map(|_| None),
        enum_def().map(|_| None),
        extern_decl().map(|_| None),
    ));

    attr.ignore_then(keyword_or_ident())
        .then(keyword_or_ident().or_not())
        .then(type_params())
        .then(inline_value)
        .then(choice((
            block_item
                .repeated()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
            clause().map(Some).repeated(),
        )))
        .map(|((((kind, name), _tps), _value), items)| Decl::Block {
            kind,
            name: name.unwrap_or_default(),
            body: items.into_iter().flatten().collect(),
        })
}

// --- Top-level ---

fn decl() -> impl Parser<Token, Spanned<Decl>, Error = Simple<Token>> + Clone {
    choice((
        contract_decl().map(Decl::Contract),
        service_decl().map(Decl::Service),
        type_def().map(Decl::TypeDef),
        enum_def().map(Decl::EnumDef),
        extern_decl().map(Decl::Extern),
        fn_def().map(Decl::FnDef),
        generic_block(),
    ))
    .map_with_span(|node, span| Spanned { node, span })
}

// --- Module / Import / Project ---

fn project_decl() -> impl Parser<Token, ProjectDecl, Error = Simple<Token>> + Clone {
    just(Token::Project)
        .ignore_then(ident())
        .then(
            just(Token::Profile)
                .ignore_then(just(Token::Colon))
                .ignore_then(
                    ident()
                        .separated_by(just(Token::Comma))
                        .delimited_by(just(Token::LBracket), just(Token::RBracket)),
                )
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map(|(name, profile)| ProjectDecl { name, profile })
}

fn module_decl() -> impl Parser<Token, ModuleDecl, Error = Simple<Token>> + Clone {
    just(Token::Module)
        .ignore_then(dotted_path())
        .then_ignore(just(Token::Semicolon))
        .map(|path| ModuleDecl { path })
}

fn import_decl() -> impl Parser<Token, ImportDecl, Error = Simple<Token>> + Clone {
    just(Token::Import)
        .ignore_then(dotted_path())
        .then(just(Token::As).ignore_then(ident()).or_not())
        .then(
            ident()
                .separated_by(just(Token::Comma))
                .delimited_by(just(Token::LBrace), just(Token::RBrace))
                .or_not(),
        )
        .then_ignore(just(Token::Semicolon).or_not())
        .map(|((path, alias), items)| ImportDecl {
            path,
            alias,
            items: items.unwrap_or_default(),
        })
}

pub fn source_file() -> impl Parser<Token, SourceFile, Error = Simple<Token>> {
    project_decl()
        .or_not()
        .then(module_decl().or_not())
        .then(import_decl().repeated())
        .then(decl().repeated())
        .then_ignore(end())
        .map(|(((project, module), imports), decls)| SourceFile {
            project,
            module,
            imports,
            decls,
        })
}