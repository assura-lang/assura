use crate::ast::*;
use crate::cst::{self, LexedToken, TokenSpan, build_tree};
use crate::grammar;
use crate::lexer::Token;
use crate::lower::*;
use crate::syntax_kind::SyntaxKind;
use logos::Logos;

/// Parse source and lower to AST.
fn parse_and_lower(source: &str) -> (SourceFile, Vec<cst::ParseError>) {
    let lex = Token::lexer(source);
    let mut tokens = Vec::new();
    let mut spans = Vec::new();

    for (tok, span) in lex.spanned() {
        if let Ok(t) = tok {
            let kind = SyntaxKind::from(&t);
            tokens.push(LexedToken {
                kind,
                text: source[span.clone()].to_string(),
            });
            spans.push(TokenSpan {
                start: span.start,
                end: span.end,
            });
        }
    }

    let mut parser = cst::Parser::new(tokens, spans);
    grammar::source_file(&mut parser);
    let green = build_tree(parser.events, &parser.tokens);
    let node = crate::syntax_kind::SyntaxNode::new_root(green);
    let sf = lower_source_file(&node);
    (sf, parser.errors)
}

#[test]
fn lower_empty() {
    let (sf, errors) = parse_and_lower("");
    assert!(errors.is_empty());
    assert!(sf.project.is_none());
    assert!(sf.decls.is_empty());
}

#[test]
fn lower_project_module_imports() {
    let src = r#"
        project MyApp {
            profile: [safety, security]
        }
        module mymod;
        import std.io;
    "#;
    let (sf, errors) = parse_and_lower(src);
    assert!(errors.is_empty(), "errors: {errors:?}");
    assert_eq!(sf.project.as_ref().unwrap().name, "MyApp");
    assert_eq!(
        sf.project.as_ref().unwrap().profile,
        vec!["safety", "security"]
    );
    assert_eq!(sf.module.as_ref().unwrap().path, vec!["mymod"]);
    assert_eq!(sf.imports.len(), 1);
    assert_eq!(sf.imports[0].path, vec!["std", "io"]);
}

#[test]
fn lower_contract_with_clauses() {
    let src = r#"
        contract Foo {
            requires n > 0
            ensures result >= 0
        }
    "#;
    let (sf, errors) = parse_and_lower(src);
    assert!(errors.is_empty(), "errors: {errors:?}");
    assert_eq!(sf.decls.len(), 1);
    if let Decl::Contract(c) = &sf.decls[0].node {
        assert_eq!(c.name, "Foo");
        assert_eq!(c.clauses.len(), 2);
        assert_eq!(c.clauses[0].kind, ClauseKind::Requires);
        assert_eq!(c.clauses[1].kind, ClauseKind::Ensures);
    } else {
        panic!("expected Contract");
    }
}

#[test]
fn lower_contract_with_inline_fn_params() {
    let src = r#"
        contract Bad {
            requires x > 0
            fn bad(x: Int, y: Float) -> Int
        }
    "#;
    let (sf, errors) = parse_and_lower(src);
    assert!(errors.is_empty(), "errors: {errors:?}");
    assert_eq!(sf.decls.len(), 1);
    if let Decl::Contract(c) = &sf.decls[0].node {
        assert_eq!(c.name, "Bad");
        assert_eq!(
            c.fn_params.len(),
            2,
            "fn_params should have 2 params, got: {:?}",
            c.fn_params
        );
        assert_eq!(c.fn_params[0].name, "x");
        assert_eq!(c.fn_params[1].name, "y");
    } else {
        panic!("expected Contract");
    }
}

#[test]
fn lower_fn_with_clauses() {
    let src = r#"
        fn factorial(n: Nat) -> Nat
            requires n >= 0
            decreases n
            ensures result >= 1
    "#;
    let (sf, errors) = parse_and_lower(src);
    assert!(errors.is_empty(), "errors: {errors:?}");
    assert_eq!(sf.decls.len(), 1);
    if let Decl::FnDef(f) = &sf.decls[0].node {
        assert_eq!(f.name, "factorial");
        assert_eq!(f.params.len(), 1);
        assert_eq!(f.params[0].name, "n");
        assert_eq!(
            f.params[0].ty,
            Some(crate::ast::TypeExpr::Named("Nat".into()))
        );
        assert_eq!(f.return_ty, Some(crate::ast::TypeExpr::Named("Nat".into())));
        assert_eq!(f.clauses.len(), 3);
    } else {
        panic!("expected FnDef");
    }
}

#[test]
fn lower_type_struct() {
    let src = r#"
        type Point {
            x: Int;
            y: Int;
        }
    "#;
    let (sf, errors) = parse_and_lower(src);
    assert!(errors.is_empty(), "errors: {errors:?}");
    if let Decl::TypeDef(td) = &sf.decls[0].node {
        assert_eq!(td.name, "Point");
        if let TypeBody::Struct(fields) = &td.body {
            assert_eq!(fields.len(), 2);
            assert_eq!(fields[0].name, "x");
            assert_eq!(fields[1].name, "y");
        } else {
            panic!("expected Struct body");
        }
    } else {
        panic!("expected TypeDef");
    }
}

#[test]
fn lower_enum() {
    let src = r#"
        enum Color {
            Red,
            Green,
            Blue,
        }
    "#;
    let (sf, errors) = parse_and_lower(src);
    assert!(errors.is_empty(), "errors: {errors:?}");
    if let Decl::EnumDef(ed) = &sf.decls[0].node {
        assert_eq!(ed.name, "Color");
        assert_eq!(ed.variants.len(), 3);
        assert_eq!(ed.variants[0].name, "Red");
    } else {
        panic!("expected EnumDef");
    }
}

#[test]
fn lower_enum_variant_fields_exclude_commas() {
    let src = "enum Shape { Rect(Int, Int), Circle(Float) }";
    let (sf, errors) = parse_and_lower(src);
    assert!(errors.is_empty(), "errors: {errors:?}");
    if let Decl::EnumDef(ed) = &sf.decls[0].node {
        // Rect(Int, Int) should have exactly 2 fields, no commas
        assert_eq!(ed.variants[0].name, "Rect");
        assert_eq!(ed.variants[0].fields, vec!["Int", "Int"]);
        // Circle(Float) should have exactly 1 field
        assert_eq!(ed.variants[1].name, "Circle");
        assert_eq!(ed.variants[1].fields, vec!["Float"]);
    } else {
        panic!("expected EnumDef");
    }
}

#[test]
fn lower_expr_binary() {
    let src = r#"
        contract Foo {
            requires a + b > 0
        }
    "#;
    let (sf, errors) = parse_and_lower(src);
    assert!(errors.is_empty(), "errors: {errors:?}");
    if let Decl::Contract(c) = &sf.decls[0].node {
        // The requires body should be a BinOp expression
        match &c.clauses[0].body.node {
            Expr::BinOp { op, .. } => assert_eq!(*op, BinOp::Gt),
            other => panic!("expected BinOp, got {other:?}"),
        }
    }
}

#[test]
fn lower_expr_binary_chain_spans() {
    // For `a + b + c`, the inner `a + b` should have a span covering
    // both `a` and `b`, not 0..0.
    let src = r#"
        contract Foo {
            requires a + b + c > 0
        }
    "#;
    let (sf, errors) = parse_and_lower(src);
    assert!(errors.is_empty(), "errors: {errors:?}");
    if let Decl::Contract(c) = &sf.decls[0].node {
        // Top-level: BinOp(>, lhs=BinOp(+, ..., c), rhs=0)
        let body = &c.clauses[0].body;
        assert!(
            body.span != (0..0),
            "top-level body span should not be 0..0"
        );
        if let Expr::BinOp { lhs, .. } = &body.node {
            // lhs is `a + b + c` addition chain
            assert!(lhs.span != (0..0), "addition chain span should not be 0..0");
            if let Expr::BinOp {
                lhs: inner_lhs,
                rhs: inner_rhs,
                ..
            } = &lhs.node
            {
                // inner_lhs is `a + b`, inner_rhs is `c`
                assert!(
                    inner_lhs.span != (0..0),
                    "inner `a + b` span should not be 0..0, got {:?}",
                    inner_lhs.span
                );
                assert!(
                    inner_rhs.span != (0..0),
                    "inner `c` span should not be 0..0, got {:?}",
                    inner_rhs.span
                );
            }
        }
    }
}

#[test]
fn lower_bind_basic() {
    let src = r#"
        bind "std::cmp::max" as safe_max {
            input(a: Int, b: Int)
            output(result: Int)
            ensures result >= a
            ensures result >= b
        }
    "#;
    let (sf, errors) = parse_and_lower(src);
    assert!(errors.is_empty(), "errors: {errors:?}");
    assert_eq!(sf.decls.len(), 1);
    if let Decl::Bind(b) = &sf.decls[0].node {
        assert_eq!(b.name, "safe_max");
        assert_eq!(b.target_path, "std::cmp::max");
        assert_eq!(b.params.len(), 2);
        assert_eq!(b.params[0].name, "a");
        assert_eq!(b.params[1].name, "b");
        b.return_ty.as_ref().unwrap();
        assert_eq!(
            b.clauses.len(),
            2,
            "expected 2 ensures clauses, got {:?}",
            b.clauses
        );
        assert!(b.clauses.iter().all(|c| c.kind == ClauseKind::Ensures));
    } else {
        panic!("expected Decl::Bind, got {:?}", sf.decls[0].node);
    }
}

#[test]
fn lower_bind_with_requires() {
    let src = r#"
        bind "my_crate::divide" as safe_divide {
            input(a: Int, b: Int)
            output(result: Int)
            requires b != 0
            ensures result * b == a
        }
    "#;
    let (sf, errors) = parse_and_lower(src);
    assert!(errors.is_empty(), "errors: {errors:?}");
    if let Decl::Bind(b) = &sf.decls[0].node {
        assert_eq!(b.name, "safe_divide");
        assert_eq!(b.target_path, "my_crate::divide");
        let requires_count = b
            .clauses
            .iter()
            .filter(|c| c.kind == ClauseKind::Requires)
            .count();
        let ensures_count = b
            .clauses
            .iter()
            .filter(|c| c.kind == ClauseKind::Ensures)
            .count();
        assert_eq!(requires_count, 1);
        assert_eq!(ensures_count, 1);
    } else {
        panic!("expected Decl::Bind");
    }
}

#[test]
fn test_prophecy_decl() {
    let src = "ghost prophecy future_value: Int";
    let (file, errs) = crate::parse(src);
    assert!(errs.is_empty(), "unexpected errors: {errs:?}");
    let file = file.unwrap();
    assert_eq!(file.decls.len(), 1);
    if let Decl::Prophecy(p) = &file.decls[0].node {
        assert_eq!(p.name, "future_value");
        assert_eq!(p.ty, Some(crate::ast::TypeExpr::Named("Int".into())));
    } else {
        panic!("expected Decl::Prophecy, got {:?}", file.decls[0].node);
    }
}

#[test]
fn test_prophecy_decl_no_type() {
    let src = "ghost prophecy x";
    let (file, errs) = crate::parse(src);
    assert!(errs.is_empty(), "unexpected errors: {errs:?}");
    let file = file.unwrap();
    assert_eq!(file.decls.len(), 1);
    if let Decl::Prophecy(p) = &file.decls[0].node {
        assert_eq!(p.name, "x");
        assert!(p.ty.is_none());
    } else {
        panic!("expected Decl::Prophecy, got {:?}", file.decls[0].node);
    }
}

#[test]
fn test_prophecy_with_contract() {
    let src = r#"
ghost prophecy final_result: Int

contract UseProphecy {
requires { final_result > 0 }
}
"#;
    let (file, errs) = crate::parse(src);
    assert!(errs.is_empty(), "unexpected errors: {errs:?}");
    let file = file.unwrap();
    assert_eq!(file.decls.len(), 2);
    assert!(matches!(&file.decls[0].node, Decl::Prophecy(_)));
    assert!(matches!(&file.decls[1].node, Decl::Contract(_)));
}

#[test]
fn test_consecutive_prophecy_declarations() {
    // Regression test for #158: consecutive prophecy decls merged into one block
    let src = "module test\nprophecy alpha: Int\nprophecy beta: Int";
    let (file, errs) = crate::parse(src);
    assert!(errs.is_empty(), "unexpected errors: {errs:?}");
    let file = file.unwrap();
    // Must produce TWO separate Prophecy decls, not one merged Block
    let prophecies: Vec<_> = file
        .decls
        .iter()
        .filter(|d| matches!(&d.node, Decl::Prophecy(_)))
        .collect();
    assert_eq!(
        prophecies.len(),
        2,
        "expected 2 prophecy decls, got {}: {:#?}",
        prophecies.len(),
        file.decls
            .iter()
            .map(|d| format!("{:?}", d.node))
            .collect::<Vec<_>>()
    );
    if let Decl::Prophecy(p) = &prophecies[0].node {
        assert_eq!(p.name, "alpha");
    }
    if let Decl::Prophecy(p) = &prophecies[1].node {
        assert_eq!(p.name, "beta");
    }
}

#[test]
fn test_consecutive_ghost_prophecy_declarations() {
    // ghost prophecy form should also work consecutively
    let src = "ghost prophecy a: Int\nghost prophecy b: Float";
    let (file, errs) = crate::parse(src);
    assert!(errs.is_empty(), "unexpected errors: {errs:?}");
    let file = file.unwrap();
    let prophecies: Vec<_> = file
        .decls
        .iter()
        .filter(|d| matches!(&d.node, Decl::Prophecy(_)))
        .collect();
    assert_eq!(prophecies.len(), 2);
    if let Decl::Prophecy(p) = &prophecies[0].node {
        assert_eq!(p.name, "a");
    }
    if let Decl::Prophecy(p) = &prophecies[1].node {
        assert_eq!(p.name, "b");
    }
}

#[test]
fn test_liveness_block() {
    let src = r#"
liveness Progress {
assume: fair
prove: eventually(turn == 1)
}
"#;
    let (file, errs) = crate::parse(src);
    assert!(errs.is_empty(), "unexpected errors: {errs:?}");
    let file = file.unwrap();
    assert_eq!(file.decls.len(), 1);
    if let Decl::Block {
        kind, name, body, ..
    } = &file.decls[0].node
    {
        assert_eq!(*kind, BlockKind::Liveness);
        assert_eq!(name, "Progress");
        assert!(
            body.len() >= 2,
            "expected assume + prove clauses, got {}",
            body.len()
        );
    } else {
        panic!("expected Decl::Block, got {:?}", file.decls[0].node);
    }
}

#[test]
fn test_liveness_block_braced_body() {
    // Regression test for #53: clause bodies with braces inside generic blocks
    let src = r#"
liveness Progress {
assume: fair
prove: eventually { turn == 1 }
}
"#;
    let (file, errs) = crate::parse(src);
    assert!(errs.is_empty(), "unexpected errors: {errs:?}");
    let file = file.unwrap();
    assert_eq!(file.decls.len(), 1);
    if let Decl::Block {
        kind, name, body, ..
    } = &file.decls[0].node
    {
        assert_eq!(*kind, BlockKind::Liveness);
        assert_eq!(name, "Progress");
        assert!(
            body.len() >= 2,
            "expected assume + prove clauses, got {}",
            body.len()
        );
    } else {
        panic!("expected Decl::Block, got {:?}", file.decls[0].node);
    }
}

#[test]
fn test_liveness_block_multiple_braced_clauses() {
    // Also covers #53: multiple brace-delimited bodies in one block
    let src = r#"
liveness Fairness {
prove: eventually { turn == 1 }
prove: eventually_within { progress == true }
}
"#;
    let (file, errs) = crate::parse(src);
    assert!(errs.is_empty(), "unexpected errors: {errs:?}");
    let file = file.unwrap();
    if let Decl::Block { body, .. } = &file.decls[0].node {
        assert!(
            body.len() >= 2,
            "expected 2 prove clauses, got {}",
            body.len()
        );
    } else {
        panic!("expected Decl::Block");
    }
}

#[test]
fn test_generic_block_value_extraction() {
    let src = "feature_max MAX_SIZE: Nat = 256";
    let (file, errs) = crate::parse(src);
    assert!(errs.is_empty(), "unexpected errors: {errs:?}");
    let file = file.unwrap();
    assert_eq!(file.decls.len(), 1);
    if let Decl::Block {
        kind, name, value, ..
    } = &file.decls[0].node
    {
        assert_eq!(*kind, BlockKind::FeatureMax);
        assert_eq!(name, "MAX_SIZE");
        let v = value.as_ref().expect("value should be Some");
        assert!(
            v.contains(&"256".to_string()),
            "value tokens should contain '256', got: {v:?}"
        );
    } else {
        panic!("expected Decl::Block, got {:?}", file.decls[0].node);
    }
}

#[test]
fn test_generic_block_equals_value() {
    let src = "feature ecdsa = enabled";
    let (file, errs) = crate::parse(src);
    assert!(errs.is_empty(), "unexpected errors: {errs:?}");
    let file = file.unwrap();
    assert_eq!(file.decls.len(), 1);
    if let Decl::Block {
        kind, name, value, ..
    } = &file.decls[0].node
    {
        assert_eq!(*kind, BlockKind::Feature);
        assert_eq!(name, "ecdsa");
        let v = value.as_ref().expect("value should be Some");
        assert!(
            v.contains(&"enabled".to_string()),
            "value tokens should contain 'enabled', got: {v:?}"
        );
    } else {
        panic!("expected Decl::Block, got {:?}", file.decls[0].node);
    }
}

#[test]
fn lower_codec_registry_basic() {
    let src = r#"
        codec_registry ImageFormats {
            output: ImageOutput,

            codec Png {
                magic: [0x89, 0x50, 0x4E, 0x47],
                decoder: decode_png
            }

            codec Bmp {
                magic: [0x42, 0x4D, ..],
                decoder: decode_bmp
            }
        }
    "#;
    let file = crate::parse_unwrap(src);
    assert_eq!(file.decls.len(), 1);
    if let Decl::CodecRegistry(cr) = &file.decls[0].node {
        assert_eq!(cr.name, "ImageFormats");
        assert_eq!(cr.output_type, vec!["ImageOutput"]);
        assert_eq!(cr.codecs.len(), 2);

        assert_eq!(cr.codecs[0].name, "Png");
        assert_eq!(cr.codecs[0].decoder, "decode_png");
        if let MagicPattern::Bytes { bytes, prefix } = &cr.codecs[0].magic {
            assert_eq!(bytes, &[0x89, 0x50, 0x4E, 0x47]);
            assert!(!prefix);
        } else {
            panic!("expected MagicPattern::Bytes for Png");
        }

        assert_eq!(cr.codecs[1].name, "Bmp");
        assert_eq!(cr.codecs[1].decoder, "decode_bmp");
        if let MagicPattern::Bytes { bytes, prefix } = &cr.codecs[1].magic {
            assert_eq!(bytes, &[0x42, 0x4D]);
            assert!(prefix, "BMP should have prefix=true due to '..'");
        } else {
            panic!("expected MagicPattern::Bytes for Bmp");
        }
    } else {
        panic!("expected Decl::CodecRegistry, got {:?}", file.decls[0].node);
    }
}

#[test]
fn lower_codec_registry_probe_and_extension() {
    let src = r#"
        codec_registry Formats {
            output: FormatOutput,

            codec Hdr {
                magic: probe(is_hdr_format),
                decoder: decode_hdr
            }

            codec Text {
                magic: extension(".txt", ".md"),
                decoder: decode_text
            }
        }
    "#;
    let file = crate::parse_unwrap(src);
    if let Decl::CodecRegistry(cr) = &file.decls[0].node {
        assert_eq!(cr.codecs.len(), 2);

        if let MagicPattern::Probe(fn_name) = &cr.codecs[0].magic {
            assert_eq!(fn_name, "is_hdr_format");
        } else {
            panic!("expected MagicPattern::Probe for Hdr");
        }

        if let MagicPattern::Extension(exts) = &cr.codecs[1].magic {
            assert_eq!(exts, &[".txt", ".md"]);
        } else {
            panic!("expected MagicPattern::Extension for Text");
        }
    } else {
        panic!("expected Decl::CodecRegistry");
    }
}

#[test]
fn lower_codec_registry_with_contracts() {
    let src = r#"
        codec_registry ImageFormats {
            output: ImageOutput,

            codec Png {
                magic: [0x89, 0x50],
                decoder: decode_png,
                contracts: {
                    ensures { result.width >= 1 }
                }
            }
        }
    "#;
    let file = crate::parse_unwrap(src);
    if let Decl::CodecRegistry(cr) = &file.decls[0].node {
        assert_eq!(cr.codecs.len(), 1);
        assert!(
            !cr.codecs[0].contracts.is_empty(),
            "contracts should be non-empty"
        );
    } else {
        panic!("expected Decl::CodecRegistry");
    }
}

#[test]
fn test_effect_row_with_variable() {
    let src = r#"
contract EffPoly {
effects <io | E>
fn map_with_effect(f: (Int) -> Int) -> List<Int>
}
"#;
    let sf = crate::parse_unwrap(src);
    assert_eq!(sf.decls.len(), 1);
    if let Decl::Contract(c) = &sf.decls[0].node {
        let eff_clause = c
            .clauses
            .iter()
            .find(|cl| cl.kind == ClauseKind::Effects)
            .expect("should have an effects clause");
        assert_eq!(
            eff_clause.effect_variables,
            vec!["E".to_string()],
            "should extract effect variable E from row"
        );
    } else {
        panic!("expected Decl::Contract");
    }
}

#[test]
fn test_effect_row_multiple_variables() {
    let src = r#"
contract MultiEff {
effects <io, net | E, F>
fn poly_fn() -> Unit
}
"#;
    let sf = crate::parse_unwrap(src);
    if let Decl::Contract(c) = &sf.decls[0].node {
        let eff_clause = c
            .clauses
            .iter()
            .find(|cl| cl.kind == ClauseKind::Effects)
            .expect("should have an effects clause");
        assert_eq!(
            eff_clause.effect_variables,
            vec!["E".to_string(), "F".to_string()],
            "should extract both effect variables E and F"
        );
    } else {
        panic!("expected Decl::Contract");
    }
}

#[test]
fn test_effect_row_no_variables() {
    let src = r#"
contract ConcreteEff {
effects <io>
fn concrete_fn() -> Unit
}
"#;
    let sf = crate::parse_unwrap(src);
    if let Decl::Contract(c) = &sf.decls[0].node {
        let eff_clause = c
            .clauses
            .iter()
            .find(|cl| cl.kind == ClauseKind::Effects)
            .expect("should have an effects clause");
        assert!(
            eff_clause.effect_variables.is_empty(),
            "effects without pipe should have no effect variables, got: {:?}",
            eff_clause.effect_variables
        );
    } else {
        panic!("expected Decl::Contract");
    }
}

#[test]
fn lower_braced_clause_body_has_correct_original_spans() {
    // Regression for #335: expressions inside `requires { ... }` (and similar)
    // must have SpExpr spans that are byte offsets into the *original* source,
    // not compressed trivia-stripped coordinates pointing inside the keyword.
    let src = r#"
contract BadExpr {
requires { 1 + true } // trivia after braced body (tests clause + expr span)
}
"#;
    let (sf, errors) = parse_and_lower(src);
    assert!(errors.is_empty(), "parse/lower errors: {errors:?}");
    if let Decl::Contract(c) = &sf.decls[0].node {
        assert!(!c.clauses.is_empty(), "expected at least one clause");
        let body = &c.clauses[0].body;
        // body span must refer to real content around the expression
        let body_text = &src[body.span.clone()];
        // Span must cover the expression content (may include trailing trivia like comments)
        assert!(
            body_text.contains("1 + true"),
            "braced body span {:?} should cover '1 + true', got {:?}",
            body.span,
            body_text
        );
        // Sub-expressions too (BinOp children) -- exact for literals where possible
        if let Expr::BinOp { lhs, rhs, .. } = &body.node {
            let lhs_text = &src[lhs.span.clone()];
            let rhs_text = &src[rhs.span.clone()];
            assert!(
                lhs_text.trim() == "1",
                "lhs span should point at literal 1, got span {:?} text {:?}",
                lhs.span,
                lhs_text
            );
            assert!(
                rhs_text.contains("true"),
                "rhs span should point at 'true', got span {:?} text {:?}",
                rhs.span,
                rhs_text
            );
        }
    } else {
        panic!("expected Contract decl");
    }
}
