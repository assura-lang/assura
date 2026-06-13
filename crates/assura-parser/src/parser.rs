use crate::ast::*;
use crate::lexer::Token;
use chumsky::BoxedParser;
use chumsky::prelude::*;

fn ident() -> impl Parser<Token, String, Error = Simple<Token>> + Clone {
    filter_map(|span, tok| match tok {
        Token::Ident(s) => Ok(s),
        _ => Err(Simple::expected_input_found(span, [], Some(tok))),
    })
}

/// Accept keyword tokens as identifiers (for extended syntax blocks, field names, etc.)
///
/// Many keywords can appear in identifier position in extended syntax blocks,
/// field definitions, or as block kind names. This function allows them there.
fn keyword_or_ident() -> impl Parser<Token, String, Error = Simple<Token>> + Clone {
    filter_map(|span, tok| match &tok {
        Token::Ident(s) => Ok(s.clone()),
        // All keywords that can appear in identifier position.
        // We accept every keyword here; the parser context determines
        // which are valid vs. which start a new production.
        _ => {
            let s = tok_to_str(&tok);
            // Reject punctuation and operators — only keywords produce valid ident strings
            if s.chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            {
                Ok(s)
            } else {
                Err(Simple::expected_input_found(span, [], Some(tok)))
            }
        }
    })
}

fn dotted_path() -> impl Parser<Token, Vec<String>, Error = Simple<Token>> + Clone {
    ident().separated_by(just(Token::Dot)).at_least(1)
}

fn type_params() -> impl Parser<Token, Vec<String>, Error = Simple<Token>> + Clone {
    // Support bounded params: <T: Trait, U: Bound>  -- keep just the names
    let bounded_param = ident()
        .then(
            just(Token::Colon)
                .ignore_then(
                    filter(|t: &Token| !matches!(t, Token::Comma | Token::RAngle))
                        .repeated()
                        .at_least(1),
                )
                .or_not(),
        )
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
        // Punctuation and operators
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
        // Keywords: map back to their source text
        Token::And => "and".into(),
        Token::ApiCompat => "api_compat".into(),
        Token::As => "as".into(),
        Token::Audit => "audit".into(),
        Token::Bind => "bind".into(),
        Token::Compliance => "compliance".into(),
        Token::Concurrency => "concurrency".into(),
        Token::Contract => "contract".into(),
        Token::DataFlow => "data-flow".into(),
        Token::Effects => "effects".into(),
        Token::Else => "else".into(),
        Token::Enum => "enum".into(),
        Token::Ensures => "ensures".into(),
        Token::Errors => "errors".into(),
        Token::Evolution => "evolution".into(),
        Token::Exists => "exists".into(),
        Token::Extern => "extern".into(),
        Token::False => "false".into(),
        Token::Fn => "fn".into(),
        Token::Forall => "forall".into(),
        Token::Idempotent => "idempotent".into(),
        Token::If => "if".into(),
        Token::Import => "import".into(),
        Token::In => "in".into(),
        Token::Input => "input".into(),
        Token::Invariant => "invariant".into(),
        Token::Is => "is".into(),
        Token::Module => "module".into(),
        Token::MustNot => "must-not".into(),
        Token::Not => "not".into(),
        Token::Observe => "observe".into(),
        Token::Old => "old".into(),
        Token::Operation => "operation".into(),
        Token::Or => "or".into(),
        Token::Output => "output".into(),
        Token::Performance => "performance".into(),
        Token::Privacy => "privacy".into(),
        Token::Profile => "profile".into(),
        Token::Project => "project".into(),
        Token::Protocol => "protocol".into(),
        Token::Pub => "pub".into(),
        Token::Pure => "pure".into(),
        Token::Query => "query".into(),
        Token::Requires => "requires".into(),
        Token::Result_ => "result".into(),
        Token::Retention => "retention".into(),
        Token::Rule => "rule".into(),
        Token::Self_ => "self".into(),
        Token::Serialization => "serialization".into(),
        Token::Service => "service".into(),
        Token::States => "states".into(),
        Token::Then => "then".into(),
        Token::Transaction => "transaction".into(),
        Token::Transition => "transition".into(),
        Token::True => "true".into(),
        Token::Type => "type".into(),
        Token::Where => "where".into(),
        // CORE
        Token::Apply => "apply".into(),
        Token::AutoTrigger => "auto_trigger".into(),
        Token::Axiom => "axiom".into(),
        Token::Cases => "cases".into(),
        Token::Define => "define".into(),
        Token::Eventually => "eventually".into(),
        Token::EventuallyAlways => "eventually_always".into(),
        Token::EventuallyWithin => "eventually_within".into(),
        Token::Fair => "fair".into(),
        Token::Ghost => "ghost".into(),
        Token::Induction => "induction".into(),
        Token::LeadsTo => "leads_to".into(),
        Token::Lemma => "lemma".into(),
        Token::Liveness => "liveness".into(),
        Token::Modifies => "modifies".into(),
        Token::Opaque => "opaque".into(),
        Token::Prophecy => "prophecy".into(),
        Token::Property => "property".into(),
        Token::Reads => "reads".into(),
        Token::Resolve => "resolve".into(),
        Token::Reveal => "reveal".into(),
        Token::Trigger => "trigger".into(),
        // MEM
        Token::Allocator => "allocator".into(),
        Token::Atomic => "atomic".into(),
        Token::AtomicLoad => "atomic_load".into(),
        Token::CircularBuffer => "circular_buffer".into(),
        Token::Layout => "layout".into(),
        Token::Region => "region".into(),
        Token::SharedMemory => "shared_memory".into(),
        Token::Slide => "slide".into(),
        Token::ValidCount => "valid_count".into(),
        Token::WritePos => "write_pos".into(),
        // TYPE
        Token::ErrorPolicy => "error_policy".into(),
        Token::Impl => "impl".into(),
        Token::Interface => "interface".into(),
        Token::MustNotMask => "must_not_mask".into(),
        Token::MustPropagate => "must_propagate".into(),
        Token::StructuralInvariant => "structural_invariant".into(),
        // SEC
        Token::Algorithm => "algorithm".into(),
        Token::AxiomSpec => "axiom_spec".into(),
        Token::CalleeGuarantees => "callee_guarantees".into(),
        Token::CallerGuarantees => "caller_guarantees".into(),
        Token::Conforms => "conforms".into(),
        Token::ConstantTime => "constant_time".into(),
        Token::Erase => "erase".into(),
        Token::ErrorConvention => "error_convention".into(),
        Token::Export => "export".into(),
        Token::Ffi => "ffi".into(),
        Token::Secret => "secret".into(),
        Token::SecureErase => "secure_erase".into(),
        Token::Spec => "spec".into(),
        // CONC
        Token::AcqRel => "acq_rel".into(),
        Token::Acquire => "acquire".into(),
        Token::Callback => "callback".into(),
        Token::Deadline => "deadline".into(),
        Token::Deterministic => "deterministic".into(),
        Token::Fence => "fence".into(),
        Token::LockOrder => "lock_order".into(),
        Token::LockRank => "lock_rank".into(),
        Token::MayCall => "may_call".into(),
        Token::Merge => "merge".into(),
        Token::MustBe => "must_be".into(),
        Token::MustNotCall => "must_not_call".into(),
        Token::MustNotReenter => "must_not_reenter".into(),
        Token::Ordering => "ordering".into(),
        Token::Relaxed => "relaxed".into(),
        Token::Release => "release".into(),
        Token::SeqCst => "seq_cst".into(),
        Token::StaleView => "stale_view".into(),
        Token::Timeout => "timeout".into(),
        Token::View => "view".into(),
        // STOR
        Token::Cache => "cache".into(),
        Token::CrashPoint => "crash_point".into(),
        Token::DurableState => "durable_state".into(),
        Token::EraseValue => "erase_value".into(),
        Token::Monotonic => "monotonic".into(),
        Token::OnCrashDuring => "on_crash_during".into(),
        Token::Pinned => "pinned".into(),
        Token::ProgIdempotent => "prog_idempotent".into(),
        Token::RecoversTo => "recovers_to".into(),
        Token::Recovery => "recovery".into(),
        Token::Snapshot => "snapshot".into(),
        Token::StorageModel => "storage_model".into(),
        // FMT
        Token::Accepts => "accepts".into(),
        Token::BitFormat => "bit_format".into(),
        Token::Bits => "bits".into(),
        Token::Codec => "codec".into(),
        Token::CodecRegistry => "codec_registry".into(),
        Token::Deviation => "deviation".into(),
        Token::EncodingMatches => "encoding_matches".into(),
        Token::Format => "format".into(),
        Token::Integrity => "integrity".into(),
        Token::Magic => "magic".into(),
        Token::Rejects => "rejects".into(),
        Token::Rfc => "rfc".into(),
        // NUM
        Token::MaxAbsError => "max_abs_error".into(),
        Token::MaxUlpError => "max_ulp_error".into(),
        Token::Precompute => "precompute".into(),
        Token::Precision => "precision".into(),
        Token::Table => "table".into(),
        Token::VerifyAgainst => "verify_against".into(),
        // PLAT
        Token::Cfg => "cfg".into(),
        Token::Feature => "feature".into(),
        Token::Limit => "limit".into(),
        Token::OnExceed => "on_exceed".into(),
        Token::Platform => "platform".into(),
        Token::Variant => "variant".into(),
        // PERF
        Token::AmortizedTime => "amortized_time".into(),
        Token::Bounds => "bounds".into(),
        Token::Complexity => "complexity".into(),
        Token::UnsafeEscape => "unsafe_escape".into(),
        // TEST
        Token::Convergence => "convergence".into(),
        Token::Equivalent => "equivalent".into(),
        Token::GenerateTests => "generate_tests".into(),
        Token::Passes => "passes".into(),
        Token::Quality => "quality".into(),
        Token::Refinement => "refinement".into(),
        // MISC
        Token::Extensible => "extensible".into(),
        Token::Frozen => "frozen".into(),
        Token::Incremental => "incremental".into(),
        Token::Yields => "yields".into(),
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
            !stoppers.contains(t) && !matches!(t, Token::RBrace | Token::RParen | Token::RBracket)
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
        just(Token::Errors).to(ClauseKind::Errors),
        just(Token::Rule).to(ClauseKind::Rule),
        just(Token::DataFlow).to(ClauseKind::DataFlow),
        just(Token::MustNot).to(ClauseKind::MustNot),
        // Keywords that now have dedicated tokens but act as clause kinds
        just(Token::Ghost).to(ClauseKind::Other("ghost".into())),
        just(Token::Spec).to(ClauseKind::Other("spec".into())),
        just(Token::Define).to(ClauseKind::Other("define".into())),
        just(Token::Property).to(ClauseKind::Other("property".into())),
        just(Token::ConstantTime).to(ClauseKind::Other("constant_time".into())),
        just(Token::MustBe).to(ClauseKind::Other("must_be".into())),
        just(Token::VerifyAgainst).to(ClauseKind::Other("verify_against".into())),
        just(Token::Reads).to(ClauseKind::Other("reads".into())),
        just(Token::Bounds).to(ClauseKind::Other("bounds".into())),
        // Remaining ident-based clause kinds (not yet keyword tokens)
        filter_map(|span, tok| match &tok {
            Token::Ident(s)
                if matches!(
                    s.as_str(),
                    "step"
                        | "resume"
                        | "assume"
                        | "prove"
                        | "validate"
                        | "taint"
                        | "verify"
                        | "example"
                        | "strategy"
                        | "promise"
                        | "bound"
                        | "writes"
                ) =>
            {
                Ok(ClauseKind::Other(s.clone()))
            }
            _ => Err(Simple::expected_input_found(span, [], Some(tok))),
        }),
    ))
}

/// Returns true if a token should stop inline/bare clause body collection.
/// This includes clause-starting keywords, declaration-starting keywords,
/// and block delimiters.
fn is_clause_stopper(t: &Token) -> bool {
    matches!(
        t,
        // Clause keywords
        Token::Requires
            | Token::Ensures
            | Token::Effects
            | Token::Invariant
            | Token::Modifies
            | Token::Input
            | Token::Output
            | Token::Errors
            | Token::Rule
            | Token::DataFlow
            | Token::MustNot
            // Block delimiters
            | Token::LBrace
            | Token::RBrace
            // Declaration-starting keywords
            | Token::Contract
            | Token::Type
            | Token::Enum
            | Token::Extern
            | Token::Fn
            | Token::Service
            | Token::Import
            | Token::Module
            | Token::Project
            | Token::Axiom
            | Token::Lemma
            // Clause-like keywords (now proper tokens)
            // NOTE: Ghost, Pure, Opaque are NOT stoppers -- they are modifiers
            // that also appear as values (e.g., `effects: pure`).
            | Token::Spec
            | Token::Define
            | Token::Property
            | Token::ConstantTime
            | Token::MustBe
            | Token::VerifyAgainst
            | Token::Reads
            | Token::Bounds
            | Token::Operation
            | Token::Query
            | Token::States
    ) || matches!(t, Token::Ident(s) if matches!(s.as_str(),
            "step" | "resume" | "assume" | "prove"
                | "validate" | "taint" | "verify"
                | "example" | "strategy" | "promise"
                | "bound" | "writes"
                | "operation" | "query" | "states"))
}

// ---------------------------------------------------------------------------
// Expression parser — full precedence climbing
// ---------------------------------------------------------------------------

/// Helper enum for postfix operations collected during parsing.
#[derive(Clone)]
enum PostfixOp {
    Field(String),
    MethodCall(String, Vec<Expr>),
    Index(Expr),
    Cast(String),
}

fn expr_parser() -> BoxedParser<'static, Token, Expr, Simple<Token>> {
    recursive(|expr: Recursive<Token, Expr, Simple<Token>>| {
        // ---- Atoms ----

        let int_lit = filter_map(|span, tok| match tok {
            Token::Int(s) => Ok(Expr::Literal(Literal::Int(s))),
            _ => Err(Simple::expected_input_found(span, [], Some(tok))),
        });

        let float_lit = filter_map(|span, tok| match tok {
            Token::Float(s) => Ok(Expr::Literal(Literal::Float(s))),
            _ => Err(Simple::expected_input_found(span, [], Some(tok))),
        });

        let string_lit = filter_map(|span, tok| match tok {
            Token::String(s) => Ok(Expr::Literal(Literal::Str(s))),
            _ => Err(Simple::expected_input_found(span, [], Some(tok))),
        });

        let bool_lit = just(Token::True)
            .to(Expr::Literal(Literal::Bool(true)))
            .or(just(Token::False).to(Expr::Literal(Literal::Bool(false))));

        let self_expr = just(Token::Self_).to(Expr::Ident("self".into()));
        let result_expr = just(Token::Result_).to(Expr::Ident("result".into()));

        // old(expr)
        let old_expr = just(Token::Old)
            .ignore_then(
                expr.clone()
                    .delimited_by(just(Token::LParen), just(Token::RParen)),
            )
            .map(|e| Expr::Old(Box::new(e)));

        // Parenthesized expression
        let paren_expr = expr
            .clone()
            .delimited_by(just(Token::LParen), just(Token::RParen))
            .map(|e| Expr::Paren(Box::new(e)));

        // List literal: [a, b, c]
        let list_expr = expr
            .clone()
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .delimited_by(just(Token::LBracket), just(Token::RBracket))
            .map(Expr::List);

        // forall var in domain: body
        let forall_expr = just(Token::Forall)
            .ignore_then(ident())
            .then_ignore(just(Token::In))
            .then(expr.clone())
            .then_ignore(just(Token::Colon))
            .then(expr.clone())
            .map(|((var, domain), body)| Expr::Forall {
                var,
                domain: Box::new(domain),
                body: Box::new(body),
            });

        // exists var in domain: body
        let exists_expr = just(Token::Exists)
            .ignore_then(ident())
            .then_ignore(just(Token::In))
            .then(expr.clone())
            .then_ignore(just(Token::Colon))
            .then(expr.clone())
            .map(|((var, domain), body)| Expr::Exists {
                var,
                domain: Box::new(domain),
                body: Box::new(body),
            });

        // if cond then expr [else expr]
        let if_expr = just(Token::If)
            .ignore_then(expr.clone())
            .then_ignore(just(Token::Then))
            .then(expr.clone())
            .then(just(Token::Else).ignore_then(expr.clone()).or_not())
            .map(|((cond, then_branch), else_branch)| Expr::If {
                cond: Box::new(cond),
                then_branch: Box::new(then_branch),
                else_branch: else_branch.map(Box::new),
            });

        // ghost { expr } — ghost block expression
        let ghost_block = just(Token::Ghost)
            .ignore_then(
                expr.clone()
                    .delimited_by(just(Token::LBrace), just(Token::RBrace)),
            )
            .map(|e| Expr::Ghost(Box::new(e)));

        // Identifier (plain)
        let ident_expr = ident().map(Expr::Ident);

        // Keywords that can appear as value-position identifiers in expressions
        let keyword_as_value = choice((
            just(Token::Pure).to("pure"),
            just(Token::Ghost).to("ghost"),
            just(Token::Opaque).to("opaque"),
            just(Token::Deterministic).to("deterministic"),
            just(Token::Atomic).to("atomic"),
            just(Token::Monotonic).to("monotonic"),
            just(Token::Secret).to("secret"),
            just(Token::Frozen).to("frozen"),
            just(Token::Pinned).to("pinned"),
            just(Token::Relaxed).to("relaxed"),
            just(Token::Recovery).to("recovery"),
            just(Token::Cache).to("cache"),
            just(Token::Snapshot).to("snapshot"),
            just(Token::Release).to("release"),
            just(Token::Acquire).to("acquire"),
            just(Token::AcqRel).to("acq_rel"),
            just(Token::SeqCst).to("seq_cst"),
            just(Token::View).to("view"),
            just(Token::Merge).to("merge"),
            just(Token::Fair).to("fair"),
            just(Token::Fence).to("fence"),
        ))
        .map(|s: &str| Expr::Ident(s.into()));

        let atom = choice((
            float_lit,
            int_lit,
            string_lit,
            bool_lit,
            self_expr,
            result_expr,
            old_expr,
            forall_expr,
            exists_expr,
            if_expr,
            paren_expr,
            list_expr,
            ghost_block,
            keyword_as_value,
            ident_expr,
        ))
        .boxed();

        // ---- Postfix: .field, .method(args), [index], as Type ----
        let field_access = just(Token::Dot)
            .ignore_then(keyword_or_ident())
            .then(
                expr.clone()
                    .separated_by(just(Token::Comma))
                    .allow_trailing()
                    .delimited_by(just(Token::LParen), just(Token::RParen))
                    .or_not(),
            )
            .map(|(name, args)| match args {
                Some(a) => PostfixOp::MethodCall(name, a),
                None => PostfixOp::Field(name),
            });

        let index_access = expr
            .clone()
            .delimited_by(just(Token::LBracket), just(Token::RBracket))
            .map(PostfixOp::Index);

        let cast = just(Token::As)
            .ignore_then(keyword_or_ident())
            .map(PostfixOp::Cast);

        let postfix_op = choice((field_access, index_access, cast));

        let postfix = atom
            .then(postfix_op.repeated())
            .foldl(|expr, op| match op {
                PostfixOp::Field(name) => Expr::Field(Box::new(expr), name),
                PostfixOp::MethodCall(name, args) => Expr::MethodCall {
                    receiver: Box::new(expr),
                    method: name,
                    args,
                },
                PostfixOp::Index(idx) => Expr::Index {
                    expr: Box::new(expr),
                    index: Box::new(idx),
                },
                PostfixOp::Cast(ty) => Expr::Cast {
                    expr: Box::new(expr),
                    ty,
                },
            })
            .boxed();

        // ---- Unary prefix: not, -, ! ----
        let unary = choice((
            just(Token::Not).to(UnaryOp::Not),
            just(Token::Minus).to(UnaryOp::Neg),
            just(Token::Bang).to(UnaryOp::Not),
        ))
        .repeated()
        .then(postfix)
        .foldr(|op, expr| Expr::UnaryOp {
            op,
            expr: Box::new(expr),
        });

        // ---- Binary: multiplicative *, /, % ----
        let mul_op = choice((
            just(Token::Star).to(BinOp::Mul),
            just(Token::Slash).to(BinOp::Div),
            just(Token::Percent).to(BinOp::Mod),
        ));
        let product = unary
            .clone()
            .then(mul_op.then(unary).repeated())
            .foldl(|lhs, (op, rhs)| Expr::BinOp {
                lhs: Box::new(lhs),
                op,
                rhs: Box::new(rhs),
            })
            .boxed();

        // ---- Binary: additive +, -, ++ ----
        let add_op = choice((
            just(Token::Plus).to(BinOp::Add),
            just(Token::Minus).to(BinOp::Sub),
            just(Token::Concat).to(BinOp::Concat),
        ));
        let sum = product
            .clone()
            .then(add_op.then(product).repeated())
            .foldl(|lhs, (op, rhs)| Expr::BinOp {
                lhs: Box::new(lhs),
                op,
                rhs: Box::new(rhs),
            })
            .boxed();

        // ---- Binary: range .. ----
        let range = sum
            .clone()
            .then(just(Token::DotDot).ignore_then(sum).or_not())
            .map(|(lhs, rhs)| match rhs {
                Some(r) => Expr::BinOp {
                    lhs: Box::new(lhs),
                    op: BinOp::Range,
                    rhs: Box::new(r),
                },
                None => lhs,
            })
            .boxed();

        // ---- Binary: comparison ==, !=, <, <=, >, >=, in, not in, is ----
        let cmp_op = choice((
            just(Token::Eq).to(BinOp::Eq),
            just(Token::Neq).to(BinOp::Neq),
            just(Token::Lte).to(BinOp::Lte),
            just(Token::Gte).to(BinOp::Gte),
            just(Token::LAngle).to(BinOp::Lt),
            just(Token::RAngle).to(BinOp::Gt),
            just(Token::Not)
                .ignore_then(just(Token::In))
                .to(BinOp::NotIn),
            just(Token::In).to(BinOp::In),
            just(Token::Is).to(BinOp::Eq), // `is` treated as equality check
        ));
        let comparison = range
            .clone()
            .then(cmp_op.then(range).repeated())
            .foldl(|lhs, (op, rhs)| Expr::BinOp {
                lhs: Box::new(lhs),
                op,
                rhs: Box::new(rhs),
            })
            .boxed();

        // ---- Binary: logical and, && ----
        let and_op = just(Token::And)
            .to(BinOp::And)
            .or(just(Token::AndAnd).to(BinOp::And));
        let logical_and = comparison
            .clone()
            .then(and_op.then(comparison).repeated())
            .foldl(|lhs, (op, rhs)| Expr::BinOp {
                lhs: Box::new(lhs),
                op,
                rhs: Box::new(rhs),
            })
            .boxed();

        // ---- Binary: logical or, || ----
        let or_op = just(Token::Or)
            .to(BinOp::Or)
            .or(just(Token::OrOr).to(BinOp::Or));
        let logical_or = logical_and
            .clone()
            .then(or_op.then(logical_and).repeated())
            .foldl(|lhs, (op, rhs)| Expr::BinOp {
                lhs: Box::new(lhs),
                op,
                rhs: Box::new(rhs),
            })
            .boxed();

        // ---- Binary: implies => ----
        logical_or
            .clone()
            .then(just(Token::FatArrow).ignore_then(logical_or).or_not())
            .map(|(lhs, rhs)| match rhs {
                Some(r) => Expr::BinOp {
                    lhs: Box::new(lhs),
                    op: BinOp::Implies,
                    rhs: Box::new(r),
                },
                None => lhs,
            })
    })
    .boxed()
}

fn clause_body() -> impl Parser<Token, Expr, Error = Simple<Token>> + Clone {
    // Braced bodies: optional colon, then { expr } or { raw tokens }.
    // Try expression first since braces provide clear delimiters.
    let braced_expr = just(Token::Colon)
        .or_not()
        .ignore_then(expr_parser().delimited_by(just(Token::LBrace), just(Token::RBrace)));
    let braced_raw = just(Token::Colon).or_not().ignore_then(
        body_tokens(CLAUSE_STOPS)
            .delimited_by(just(Token::LBrace), just(Token::RBrace))
            .map(Expr::Raw),
    );

    // Parened bodies: optional colon, then ( expr ) or ( raw tokens ).
    let parened_expr = just(Token::Colon)
        .or_not()
        .ignore_then(expr_parser().delimited_by(just(Token::LParen), just(Token::RParen)));
    let parened_raw = just(Token::Colon).or_not().ignore_then(
        body_tokens(CLAUSE_STOPS)
            .delimited_by(just(Token::LParen), just(Token::RParen))
            .map(Expr::Raw),
    );

    // Inline: colon then tokens until next clause keyword.
    // Raw first: the greedy raw-token approach matches the old parser
    // behavior, consuming everything up to a clause stopper. Expression
    // parsing for unbounded inline bodies can over- or under-consume
    // (e.g., `effects: pure incremental Foo` was one clause body before).
    let inline_raw = just(Token::Colon).ignore_then(
        filter(move |t: &Token| !is_clause_stopper(t))
            .map(|t| tok_to_str(&t))
            .repeated()
            .map(Expr::Raw),
    );

    // Bare: no colon, raw tokens until next clause/decl keyword.
    let bare_raw = filter(move |t: &Token| !is_clause_stopper(t))
        .map(|t| tok_to_str(&t))
        .repeated()
        .at_least(1)
        .map(Expr::Raw);

    choice((
        braced_expr,
        braced_raw,
        parened_expr,
        parened_raw,
        inline_raw,
        bare_raw,
    ))
}

fn clause() -> impl Parser<Token, Clause, Error = Simple<Token>> + Clone {
    clause_kind()
        .then(clause_body())
        .map(|(kind, body)| Clause { kind, body })
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
        .then(
            contract_item
                .repeated()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
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
    let modifiers = choice((just(Token::Ghost), just(Token::Pure), just(Token::Opaque))).repeated();

    // Skip optional `var` after ghost
    let var_kw = filter_map(|span, tok| match &tok {
        Token::Ident(s) if s == "var" => Ok(()),
        _ => Err(Simple::expected_input_found(span, [], Some(tok))),
    })
    .or_not();

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
            !matches!(
                t,
                Token::Semicolon | Token::Comma | Token::RBrace | Token::RParen | Token::RBracket
            )
        })
        .map(|t| vec![tok_to_str(&t)]);

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
        .map(|((((_is_pub, _mods), _var), name), ty)| FieldDef {
            name,
            ty,
            is_pub: _is_pub,
        })
}

fn type_def() -> impl Parser<Token, TypeDef, Error = Simple<Token>> + Clone {
    just(Token::Type)
        .ignore_then(ident())
        .then(type_params())
        .then(choice((
            // Refined: = { ... }
            just(Token::Equals)
                .ignore_then(
                    body_tokens(CLAUSE_STOPS)
                        .delimited_by(just(Token::LBrace), just(Token::RBrace)),
                )
                .map(TypeBody::Refined),
            // Alias: = SomeType (stop at decl keywords and braces)
            just(Token::Equals)
                .ignore_then(
                    filter(|t: &Token| {
                        !matches!(
                            t,
                            Token::Semicolon
                                | Token::LBrace
                                | Token::RBrace
                                | Token::Contract
                                | Token::Type
                                | Token::Enum
                                | Token::Extern
                                | Token::Fn
                                | Token::Service
                                | Token::Import
                                | Token::Module
                                | Token::Project
                                | Token::Axiom
                                | Token::Lemma
                                | Token::Requires
                                | Token::Ensures
                                | Token::Effects
                                | Token::Invariant
                                | Token::Modifies
                                | Token::Input
                                | Token::Output
                                | Token::Rule
                                | Token::DataFlow
                                | Token::MustNot
                        )
                    })
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
        .then(
            enum_variant()
                .repeated()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
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
            !matches!(
                t,
                Token::Comma | Token::RParen | Token::RBrace | Token::RBracket | Token::RAngle
            )
        })
        .map(|t| vec![tok_to_str(&t)]);

        choice((
            balanced_braces,
            balanced_parens,
            balanced_angles,
            balanced_brackets,
            single,
        ))
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
                Token::LBrace
                    | Token::Requires
                    | Token::Ensures
                    | Token::Effects
                    | Token::Modifies
                    | Token::Invariant
                    | Token::Input
                    | Token::Output
                    | Token::Rule
                    | Token::DataFlow
                    | Token::MustNot
                    | Token::MustBe
                    | Token::Bounds
            ) && !matches!(t, Token::Ident(s) if matches!(s.as_str(),
                    "promise" | "bound"))
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

    // Optional modifiers: pure, ghost, opaque — collect them to detect ghost fns
    let modifiers = choice((just(Token::Pure), just(Token::Ghost), just(Token::Opaque))).repeated();

    // fn, axiom, lemma all have function-like syntax
    let fn_keyword = choice((just(Token::Fn), just(Token::Axiom), just(Token::Lemma)));

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
                Token::LBrace
                    | Token::RBrace
                    | Token::Requires
                    | Token::Ensures
                    | Token::Effects
                    | Token::Modifies
                    | Token::Equals
                    | Token::Invariant
                    | Token::Input
                    | Token::Output
                    | Token::Rule
                    | Token::DataFlow
                    | Token::MustNot
                    | Token::MustBe
                    | Token::Bounds
            ) && !matches!(t, Token::Ident(s) if matches!(s.as_str(),
                    "promise" | "bound"))
        })
        .map(|t| vec![tok_to_str(&t)]);

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
            body_tokens(CLAUSE_STOPS).delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .or_not();

    attr.ignore_then(modifiers)
        .then_ignore(fn_keyword)
        .then(ident())
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
        .map(
            |(((((((mods, name), _tps), params), ret), _eq_body), clauses), _body)| {
                let is_ghost = mods.iter().any(|t| matches!(t, Token::Ghost));
                FnDef {
                    name,
                    is_ghost,
                    params,
                    return_ty: ret.unwrap_or_default(),
                    clauses,
                }
            },
        )
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
            .then(
                clause()
                    .repeated()
                    .delimited_by(just(Token::LBrace), just(Token::RBrace)),
            )
            .map(|(name, clauses)| ServiceItem::Operation { name, clauses }),
        just(Token::Query)
            .ignore_then(ident())
            .then(
                clause()
                    .repeated()
                    .delimited_by(just(Token::LBrace), just(Token::RBrace)),
            )
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
        .then(
            service_item()
                .repeated()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
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
            !matches!(t, Token::RParen | Token::RBracket) && !is_clause_stopper(t)
        })
        .map(|t| vec![tok_to_str(&t)]);

        choice((balanced_braces, balanced_parens, balanced_brackets, single))
            .repeated()
            .flatten()
    });

    let inline_value = choice((
        just(Token::Colon).ignore_then(value_tokens.clone()),
        just(Token::Equals).ignore_then(value_tokens),
    ))
    .or_not();

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
