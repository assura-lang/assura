use super::*;
use assura_ast::Spanned;

// -----------------------------------------------------------------------
// T043 CORE.1: Ghost code erasure tests
// -----------------------------------------------------------------------

#[test]
fn ghost_fn_produces_no_output() {
    // A ghost function should be completely erased in generated code.
    let project =
        codegen_ok("ghost fn spec_helper(x: Int) -> Bool\n    ensures { result == true }\n");
    let lib = &project.files[0].1;
    assert!(
        !lib.contains("fn spec_helper"),
        "ghost fn should not appear in generated Rust code"
    );
}

#[test]
fn non_ghost_fn_still_generated() {
    // A normal (non-ghost) function should still be generated.
    let project = codegen_ok("fn normal_helper(x: Int) -> Int\n    ensures { result >= 0 }\n");
    let lib = &project.files[0].1;
    assert!(
        lib.contains("fn normal_helper"),
        "non-ghost fn should appear in generated Rust code"
    );
}

#[test]
fn ghost_block_erased_in_expr() {
    // A ghost block expression should produce erased output.
    let expr = Spanned::no_span(Expr::Ghost(Box::new(Spanned::no_span(Expr::Literal(
        Literal::Bool(true),
    )))));
    let rust = expr_to_rust(&expr);
    assert!(
        rust.contains("ghost erased"),
        "ghost block should generate erased marker, got: {rust}"
    );
}

// -----------------------------------------------------------------------
// T044 CORE.2: Lemma erasure tests
// -----------------------------------------------------------------------

#[test]
fn lemma_fn_produces_no_output() {
    // A lemma function should be completely erased in generated code.
    let project = codegen_ok("lemma add_comm(a: Int, b: Int)\n    ensures { a + b == b + a }\n");
    let lib = &project.files[0].1;
    assert!(
        !lib.contains("fn add_comm"),
        "lemma fn should not appear in generated Rust code"
    );
}

#[test]
fn apply_expr_erased_in_codegen() {
    // apply lemma_name(args) should produce a comment, not code.
    let expr = Spanned::no_span(Expr::Apply {
        lemma_name: "my_lemma".into(),
        args: vec![Spanned::no_span(Expr::Literal(Literal::Int("42".into())))],
    });
    let rust = expr_to_rust(&expr);
    assert!(
        rust.contains("lemma my_lemma applied"),
        "apply should generate erased comment, got: {rust}"
    );
}

#[test]
fn match_expr_codegen() {
    // match expression should generate Rust match syntax
    let expr = Spanned::no_span(Expr::Match {
        scrutinee: Box::new(Spanned::no_span(Expr::Ident("status".into()))),
        arms: vec![
            assura_ast::MatchArm {
                pattern: assura_ast::Pattern::Ident("Active".into()),
                body: Spanned::no_span(Expr::Literal(Literal::Int("1".into()))),
            },
            assura_ast::MatchArm {
                pattern: assura_ast::Pattern::Wildcard,
                body: Spanned::no_span(Expr::Literal(Literal::Int("0".into()))),
            },
        ],
    });
    let rust = expr_to_rust(&expr);
    assert!(
        rust.contains("match status"),
        "should have match keyword: {rust}"
    );
    assert!(
        rust.contains("Active => 1"),
        "should have Active arm: {rust}"
    );
    assert!(rust.contains("_ => 0"), "should have wildcard arm: {rust}");
}

#[test]
fn match_without_wildcard_gets_fallback() {
    // match with only Constructor patterns (no wildcard) should get _ => unreachable!()
    let expr = Spanned::no_span(Expr::Match {
        scrutinee: Box::new(Spanned::no_span(Expr::Ident("color".into()))),
        arms: vec![
            assura_ast::MatchArm {
                pattern: assura_ast::Pattern::Constructor {
                    name: "Red".into(),
                    fields: vec![],
                },
                body: Spanned::no_span(Expr::Literal(Literal::Int("1".into()))),
            },
            assura_ast::MatchArm {
                pattern: assura_ast::Pattern::Constructor {
                    name: "Blue".into(),
                    fields: vec![],
                },
                body: Spanned::no_span(Expr::Literal(Literal::Int("2".into()))),
            },
        ],
    });
    let rust = expr_to_rust(&expr);
    assert!(
        rust.contains("_ => unreachable!"),
        "match without wildcard should get fallback: {rust}"
    );
}

#[test]
fn non_lemma_fn_still_generated() {
    // A normal (non-lemma) function should still be generated.
    let project = codegen_ok("fn helper(n: Int) -> Int\n    ensures { result >= 0 }\n");
    let lib = &project.files[0].1;
    assert!(
        lib.contains("fn helper"),
        "non-lemma fn should appear in generated Rust code"
    );
}
