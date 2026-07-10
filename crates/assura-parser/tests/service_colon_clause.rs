//! Regression: service op `requires: state == X` must parse as one clause.

#[test]
fn service_colon_requires_parses_comparison() {
    let src = r#"
service S {
  states: A -> B
  operation Op {
    requires: state == A
    ensures: state == B
  }
}
"#;
    let (file, errs) = assura_parser::parse(src);
    assert!(errs.is_empty(), "parse errors: {errs:?}");
    let file = file.expect("ast");
    let assura_parser::ast::Decl::Service(svc) = &file.decls[0].node else {
        panic!("expected service");
    };
    let assura_parser::ast::ServiceItem::Operation { clauses, .. } = &svc.items[1] else {
        panic!("expected operation, items={:?}", svc.items);
    };
    assert_eq!(
        clauses.len(),
        2,
        "expected requires+ensures only, got {clauses:?}"
    );
    assert!(
        matches!(clauses[0].kind, assura_parser::ast::ClauseKind::Requires),
        "first clause should be requires: {:?}",
        clauses[0].kind
    );
    assert!(
        !matches!(clauses[0].body.node, assura_parser::ast::Expr::Raw(ref t) if t.is_empty()),
        "requires body must not be empty Raw: {:?}",
        clauses[0].body.node
    );
    assert!(
        matches!(clauses[0].body.node, assura_parser::ast::Expr::BinOp { .. }),
        "requires body should be comparison: {:?}",
        clauses[0].body.node
    );
}
