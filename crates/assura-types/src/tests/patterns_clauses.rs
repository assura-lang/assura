use super::*;
use assura_parser::ast::Spanned;
// T017: Pattern exhaustiveness checking tests
// -----------------------------------------------------------------------

#[test]
fn exhaustive_all_variants_covered() {
    let variants = vec!["Red".into(), "Green".into(), "Blue".into()];
    let patterns = vec![
        Pattern::Variant("Red".into()),
        Pattern::Variant("Green".into()),
        Pattern::Variant("Blue".into()),
    ];
    assert_eq!(check_exhaustiveness(&patterns, &variants), None);
}

#[test]
fn exhaustive_wildcard_covers_all() {
    let variants = vec!["Red".into(), "Green".into(), "Blue".into()];
    let patterns = vec![Pattern::Wildcard];
    assert_eq!(check_exhaustiveness(&patterns, &variants), None);
}

#[test]
fn exhaustive_wildcard_with_explicit() {
    let variants = vec!["Red".into(), "Green".into(), "Blue".into()];
    let patterns = vec![Pattern::Variant("Red".into()), Pattern::Wildcard];
    assert_eq!(check_exhaustiveness(&patterns, &variants), None);
}

#[test]
fn non_exhaustive_missing_one() {
    let variants = vec!["Red".into(), "Green".into(), "Blue".into()];
    let patterns = vec![
        Pattern::Variant("Red".into()),
        Pattern::Variant("Green".into()),
    ];
    let missing = check_exhaustiveness(&patterns, &variants);
    assert_eq!(missing, Some(vec!["Blue".into()]));
}

#[test]
fn non_exhaustive_missing_multiple() {
    let variants = vec!["Red".into(), "Green".into(), "Blue".into(), "Yellow".into()];
    let patterns = vec![Pattern::Variant("Green".into())];
    let missing = check_exhaustiveness(&patterns, &variants).unwrap();
    assert_eq!(missing, vec!["Red", "Blue", "Yellow"]);
}

#[test]
fn non_exhaustive_empty_patterns() {
    let variants = vec!["A".into(), "B".into(), "C".into()];
    let patterns: Vec<Pattern> = vec![];
    let missing = check_exhaustiveness(&patterns, &variants).unwrap();
    assert_eq!(missing, vec!["A", "B", "C"]);
}

#[test]
fn exhaustive_empty_enum() {
    let variants: Vec<String> = vec![];
    let patterns: Vec<Pattern> = vec![];
    assert_eq!(check_exhaustiveness(&patterns, &variants), None);
}

#[test]
fn exhaustive_duplicate_patterns_ignored() {
    let variants = vec!["X".into(), "Y".into()];
    let patterns = vec![
        Pattern::Variant("X".into()),
        Pattern::Variant("X".into()),
        Pattern::Variant("Y".into()),
    ];
    assert_eq!(check_exhaustiveness(&patterns, &variants), None);
}

#[test]
fn non_exhaustive_literal_does_not_cover_variant() {
    let variants = vec!["Red".into(), "Green".into()];
    let patterns = vec![
        Pattern::Variant("Red".into()),
        Pattern::Literal(AstLit::Int("42".into())),
    ];
    let missing = check_exhaustiveness(&patterns, &variants).unwrap();
    assert_eq!(missing, vec!["Green"]);
}

#[test]
fn exhaustive_single_variant_enum() {
    let variants = vec!["Only".into()];
    let patterns = vec![Pattern::Variant("Only".into())];
    assert_eq!(check_exhaustiveness(&patterns, &variants), None);
}

#[test]
fn non_exhaustive_preserves_declaration_order() {
    let variants = vec![
        "Alpha".into(),
        "Beta".into(),
        "Gamma".into(),
        "Delta".into(),
        "Epsilon".into(),
    ];
    let patterns = vec![
        Pattern::Variant("Beta".into()),
        Pattern::Variant("Delta".into()),
    ];
    let missing = check_exhaustiveness(&patterns, &variants).unwrap();
    assert_eq!(missing, vec!["Alpha", "Gamma", "Epsilon"]);
}

// -----------------------------------------------------------------------
// T018: Contract clause type checking tests
// -----------------------------------------------------------------------

use assura_parser::ast::ClauseKind as AstClauseKind;

#[test]
fn clause_requires_bool_body_ok() {
    let env = TypeEnv::new();
    let body = Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)));
    let mut errors = Vec::new();
    check_clause_expr(&AstClauseKind::Requires, &body, &env, &mut errors, &(0..0));
    assert!(errors.is_empty());
}

#[test]
fn clause_requires_int_body_error() {
    let env = TypeEnv::new();
    let body = Spanned::no_span(AstExpr::Literal(AstLit::Int("42".into())));
    let mut errors = Vec::new();
    check_clause_expr(&AstClauseKind::Requires, &body, &env, &mut errors, &(0..0));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A03006");
    assert!(errors[0].message.contains("requires"));
    assert!(errors[0].message.contains("Bool"));
    assert!(errors[0].message.contains("Int"));
}

#[test]
fn clause_ensures_bool_body_ok() {
    let env = TypeEnv::new();
    let body = Spanned::no_span(AstExpr::Literal(AstLit::Bool(false)));
    let mut errors = Vec::new();
    check_clause_expr(&AstClauseKind::Ensures, &body, &env, &mut errors, &(0..0));
    assert!(errors.is_empty());
}

#[test]
fn clause_ensures_string_body_error() {
    let env = TypeEnv::new();
    let body = Spanned::no_span(AstExpr::Literal(AstLit::Str("hello".into())));
    let mut errors = Vec::new();
    check_clause_expr(&AstClauseKind::Ensures, &body, &env, &mut errors, &(0..0));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A03006");
    assert!(errors[0].message.contains("ensures"));
}

#[test]
fn clause_invariant_bool_body_ok() {
    let env = TypeEnv::new();
    let body = Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)));
    let mut errors = Vec::new();
    check_clause_expr(&AstClauseKind::Invariant, &body, &env, &mut errors, &(0..0));
    assert!(errors.is_empty());
}

#[test]
fn clause_invariant_float_body_error() {
    let env = TypeEnv::new();
    let body = Spanned::no_span(AstExpr::Literal(AstLit::Float("3.14".into())));
    let mut errors = Vec::new();
    check_clause_expr(&AstClauseKind::Invariant, &body, &env, &mut errors, &(0..0));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A03006");
    assert!(errors[0].message.contains("invariant"));
}

#[test]
fn clause_rule_bool_body_ok() {
    let env = TypeEnv::new();
    let body = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
        op: AstBinOp::And,
        rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(false)))),
    });
    let mut errors = Vec::new();
    check_clause_expr(&AstClauseKind::Rule, &body, &env, &mut errors, &(0..0));
    assert!(errors.is_empty());
}

#[test]
fn clause_rule_int_body_error() {
    let env = TypeEnv::new();
    let body = Spanned::no_span(AstExpr::Literal(AstLit::Int("99".into())));
    let mut errors = Vec::new();
    check_clause_expr(&AstClauseKind::Rule, &body, &env, &mut errors, &(0..0));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A03006");
    assert!(errors[0].message.contains("rule"));
}

#[test]
fn clause_effects_any_body_ok() {
    let env = TypeEnv::new();
    // Effects clause accepts any type (lenient)
    let body = Spanned::no_span(AstExpr::Ident("pure".into()));
    let mut errors = Vec::new();
    check_clause_expr(&AstClauseKind::Effects, &body, &env, &mut errors, &(0..0));
    assert!(errors.is_empty());
}

#[test]
fn clause_modifies_any_body_ok() {
    let env = TypeEnv::new();
    let body = Spanned::no_span(AstExpr::Ident("buffer".into()));
    let mut errors = Vec::new();
    check_clause_expr(&AstClauseKind::Modifies, &body, &env, &mut errors, &(0..0));
    assert!(errors.is_empty());
}

#[test]
fn clause_unknown_body_no_error() {
    let env = TypeEnv::new();
    // Unknown ident in requires clause should not emit A03006
    let body = Spanned::no_span(AstExpr::Ident("unknown_predicate".into()));
    let mut errors = Vec::new();
    check_clause_expr(&AstClauseKind::Requires, &body, &env, &mut errors, &(0..0));
    assert!(errors.is_empty());
}

#[test]
fn clause_comparison_in_requires_ok() {
    let mut env = TypeEnv::new();
    env.insert("x".into(), Type::Int);
    // x > 0 should infer as Bool, valid in requires
    let body = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        op: AstBinOp::Gt,
        rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("0".into())))),
    });
    let mut errors = Vec::new();
    check_clause_expr(&AstClauseKind::Requires, &body, &env, &mut errors, &(0..0));
    assert!(errors.is_empty());
}

#[test]
fn clause_requires_int_body_integration() {
    // Integration test: a contract whose requires clause has an Int body
    // should produce an A03006 error through the full type_check pipeline.
    let src = r#"
contract Bad {
  requires { 42 }
}
"#;
    let resolved = resolve_ok(src);
    let result = type_check(&resolved);
    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert!(errors.iter().any(|e| e.code == "A03006"));
}

#[test]
fn clause_requires_bool_integration() {
    // A contract with a Bool requires clause should type-check fine.
    let src = r#"
contract Good {
  requires { true }
}
"#;
    let resolved = resolve_ok(src);
    type_check(&resolved).expect("should type-check successfully");
}

#[test]
fn demo_files_type_check() {
    // Verify all demo files still type-check without errors
    for path in [
        "demos/libwebp-huffman.assura",
        "demos/zlib-inflate.assura",
        "demos/mbedtls-x509.assura",
        "tests/fixtures/test_basic.assura",
    ] {
        let full = format!(
            "{}/{}",
            env!("CARGO_MANIFEST_DIR")
                .strip_suffix("/crates/assura-types")
                .unwrap_or(env!("CARGO_MANIFEST_DIR")),
            path
        );
        // Try the workspace root path
        let content = match std::fs::read_to_string(&full) {
            Ok(c) => c,
            Err(_) => {
                // Try from two levels up (crates/assura-types -> workspace root)
                let alt = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .and_then(|p| p.parent())
                    .unwrap()
                    .join(path);
                std::fs::read_to_string(alt).unwrap_or_else(|e| panic!("cannot read {path}: {e}"))
            }
        };
        let (file, parse_errs) = assura_parser::parse(&content);
        assert!(
            parse_errs.is_empty(),
            "{path}: unexpected parse errors: {parse_errs:?}"
        );
        let file = file.unwrap_or_else(|| panic!("{path}: parse returned None"));
        let resolved = assura_resolve::resolve(&file)
            .unwrap_or_else(|e| panic!("{path}: resolve errors: {e:?}"));
        type_check(&resolved).unwrap_or_else(|e| panic!("{path}: type_check errors: {e:?}"));
    }
}

// -----------------------------------------------------------------------
