use crate::*;

// -------------------------------------------------------------------
// DeclVisitor / walk_decls tests
// -------------------------------------------------------------------

#[test]
fn decl_visitor_visits_contract() {
    struct Counter {
        contracts: usize,
    }
    impl DeclVisitor for Counter {
        fn visit_contract(&mut self, _c: &ContractDecl) {
            self.contracts += 1;
        }
    }
    let decls = vec![
        Spanned::no_span(Decl::Contract(ContractDecl {
            name: "A".into(),
            type_params: vec![],
            clauses: vec![],
            fn_params: vec![],
        })),
        Spanned::no_span(Decl::Contract(ContractDecl {
            name: "B".into(),
            type_params: vec![],
            clauses: vec![],
            fn_params: vec![],
        })),
    ];
    let mut counter = Counter { contracts: 0 };
    walk_decls(&mut counter, &decls);
    assert_eq!(counter.contracts, 2);
}

#[test]
fn decl_visitor_dispatches_fn_def() {
    struct FnCollector {
        names: Vec<String>,
    }
    impl DeclVisitor for FnCollector {
        fn visit_fn_def(&mut self, f: &FnDef) {
            self.names.push(f.name.clone());
        }
    }
    let decls = vec![Spanned::no_span(Decl::FnDef(FnDef {
        name: "my_fn".into(),
        is_ghost: false,
        is_lemma: false,
        params: vec![],
        return_ty: None,
        clauses: vec![],
    }))];
    let mut collector = FnCollector { names: vec![] };
    walk_decls(&mut collector, &decls);
    assert_eq!(collector.names, vec!["my_fn"]);
}

#[test]
fn decl_visitor_mixed_decl_types() {
    struct TypeCounter {
        contracts: usize,
        externs: usize,
        fns: usize,
    }
    impl DeclVisitor for TypeCounter {
        fn visit_contract(&mut self, _c: &ContractDecl) {
            self.contracts += 1;
        }
        fn visit_extern(&mut self, _e: &ExternDecl) {
            self.externs += 1;
        }
        fn visit_fn_def(&mut self, _f: &FnDef) {
            self.fns += 1;
        }
    }
    let decls = vec![
        Spanned::no_span(Decl::Contract(ContractDecl {
            name: "C".into(),
            type_params: vec![],
            clauses: vec![],
            fn_params: vec![],
        })),
        Spanned::no_span(Decl::Extern(ExternDecl {
            name: "ext".into(),
            params: vec![],
            return_ty: None,
            clauses: vec![],
        })),
        Spanned::no_span(Decl::FnDef(FnDef {
            name: "f".into(),
            is_ghost: false,
            is_lemma: false,
            params: vec![],
            return_ty: None,
            clauses: vec![],
        })),
    ];
    let mut counter = TypeCounter {
        contracts: 0,
        externs: 0,
        fns: 0,
    };
    walk_decls(&mut counter, &decls);
    assert_eq!(counter.contracts, 1);
    assert_eq!(counter.externs, 1);
    assert_eq!(counter.fns, 1);
}

// -------------------------------------------------------------------
// Decl accessor methods
// -------------------------------------------------------------------

#[test]
fn decl_name_returns_correct_name() {
    let contract = Decl::Contract(ContractDecl {
        name: "SafeDiv".into(),
        type_params: vec![],
        clauses: vec![],
        fn_params: vec![],
    });
    assert_eq!(contract.name(), Some("SafeDiv"));

    let fn_def = Decl::FnDef(FnDef {
        name: "compute".into(),
        is_ghost: false,
        is_lemma: false,
        params: vec![],
        return_ty: None,
        clauses: vec![],
    });
    assert_eq!(fn_def.name(), Some("compute"));
}

#[test]
fn decl_clauses_returns_clauses() {
    let clause = Clause {
        kind: ClauseKind::Requires,
        body: Spanned::no_span(Expr::Literal(Literal::Bool(true))),
        effect_variables: vec![],
    };
    let contract = Decl::Contract(ContractDecl {
        name: "T".into(),
        type_params: vec![],
        clauses: vec![clause],
        fn_params: vec![],
    });
    assert_eq!(contract.clauses().len(), 1);
}

#[test]
fn decl_summary_label_formats_correctly() {
    let contract = Decl::Contract(ContractDecl {
        name: "MyContract".into(),
        type_params: vec![],
        clauses: vec![],
        fn_params: vec![],
    });
    assert_eq!(contract.summary_label(), "contract MyContract");

    let ext = Decl::Extern(ExternDecl {
        name: "read_file".into(),
        params: vec![],
        return_ty: None,
        clauses: vec![],
    });
    assert_eq!(ext.summary_label(), "extern read_file");
}

// -------------------------------------------------------------------
// ExprVisitor tests
// -------------------------------------------------------------------

#[test]
fn expr_visitor_counts_identifiers() {
    struct IdentCounter {
        count: usize,
    }
    impl ExprVisitor for IdentCounter {
        fn visit_ident(&mut self, _name: &str) {
            self.count += 1;
        }
    }
    // Build: x + y
    let expr = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        op: BinOp::Add,
        rhs: Box::new(Spanned::no_span(Expr::Ident("y".into()))),
    });
    let mut counter = IdentCounter { count: 0 };
    counter.visit_expr(&expr);
    assert_eq!(counter.count, 2);
}

#[test]
fn expr_visitor_collects_ident_names() {
    struct IdentCollector {
        names: Vec<String>,
    }
    impl ExprVisitor for IdentCollector {
        fn visit_ident(&mut self, name: &str) {
            self.names.push(name.to_string());
        }
    }
    // Build: if x > 0 then y else z
    let expr = Spanned::no_span(Expr::If {
        cond: Box::new(Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            op: BinOp::Gt,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        })),
        then_branch: Box::new(Spanned::no_span(Expr::Ident("y".into()))),
        else_branch: Some(Box::new(Spanned::no_span(Expr::Ident("z".into())))),
    });
    let mut collector = IdentCollector { names: vec![] };
    collector.visit_expr(&expr);
    assert_eq!(collector.names, vec!["x", "y", "z"]);
}

#[test]
fn expr_visitor_visits_method_call() {
    struct MethodCollector {
        methods: Vec<String>,
    }
    impl ExprVisitor for MethodCollector {
        fn visit_method_call(&mut self, receiver: &SpExpr, method: &str, args: &[SpExpr]) {
            self.methods.push(method.to_string());
            // Default traversal
            self.visit_expr(receiver);
            for arg in args {
                self.visit_expr(arg);
            }
        }
    }
    // Build: data.length()
    let expr = Spanned::no_span(Expr::MethodCall {
        receiver: Box::new(Spanned::no_span(Expr::Ident("data".into()))),
        method: "length".into(),
        args: vec![],
    });
    let mut collector = MethodCollector { methods: vec![] };
    collector.visit_expr(&expr);
    assert_eq!(collector.methods, vec!["length"]);
}

// -------------------------------------------------------------------
// extract_clause_params tests (existing)
// -------------------------------------------------------------------

#[test]
fn extract_params_refined_type_with_less_than() {
    // a : { x : Int | x < 10 }, b : Bool
    // The `<` inside the refinement must NOT be treated as an angle bracket.
    let tokens: Vec<String> = vec![
        "a", ":", "{", "x", ":", "Int", "|", "x", "<", "10", "}", ",", "b", ":", "Bool",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    let body = Spanned::no_span(Expr::Raw(tokens));
    let params = extract_clause_params(&body);
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].name, "a");
    // Refined type parses as Named("{ x : Int | x < 10 }") fallback
    params[0].ty.as_ref().unwrap();
    assert_eq!(params[1].name, "b");
    assert_eq!(params[1].ty, Some(TypeExpr::Named("Bool".into())));
}

#[test]
fn extract_params_refined_type_with_parens() {
    // val : ( Int , Bool )
    let tokens: Vec<String> = vec!["val", ":", "(", "Int", ",", "Bool", ")"]
        .into_iter()
        .map(String::from)
        .collect();
    let body = Spanned::no_span(Expr::Raw(tokens));
    let params = extract_clause_params(&body);
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].name, "val");
    // Tuple-like tokens parse as Named("( Int , Bool )") fallback
    params[0].ty.as_ref().unwrap();
}

#[test]
fn extract_params_generic_type() {
    // a : List < Int >, b : Map < String , Int >
    let tokens: Vec<String> = vec![
        "a", ":", "List", "<", "Int", ">", ",", "b", ":", "Map", "<", "String", ",", "Int", ">",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    let body = Spanned::no_span(Expr::Raw(tokens));
    let params = extract_clause_params(&body);
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].name, "a");
    assert_eq!(
        params[0].ty,
        Some(TypeExpr::Generic(
            "List".into(),
            vec![TypeExpr::Named("Int".into())]
        ))
    );
    assert_eq!(params[1].name, "b");
    assert_eq!(
        params[1].ty,
        Some(TypeExpr::Generic(
            "Map".into(),
            vec![
                TypeExpr::Named("String".into()),
                TypeExpr::Named("Int".into())
            ]
        ))
    );
}

#[test]
fn negate_expr_inverts_comparisons() {
    let sp = |e| Spanned::no_span(e);

    // Eq => Neq
    let e = sp(Expr::BinOp {
        lhs: Box::new(sp(Expr::Ident("a".into()))),
        op: BinOp::Eq,
        rhs: Box::new(sp(Expr::Ident("b".into()))),
    });
    match &negate_expr(&e).node {
        Expr::BinOp { op: BinOp::Neq, .. } => {}
        other => panic!("expected Neq, got {other:?}"),
    }

    // Lt => Gte
    let e = sp(Expr::BinOp {
        lhs: Box::new(sp(Expr::Ident("x".into()))),
        op: BinOp::Lt,
        rhs: Box::new(sp(Expr::Literal(Literal::Int("0".into())))),
    });
    match &negate_expr(&e).node {
        Expr::BinOp { op: BinOp::Gte, .. } => {}
        other => panic!("expected Gte, got {other:?}"),
    }

    // In => NotIn
    let e = sp(Expr::BinOp {
        lhs: Box::new(sp(Expr::Ident("x".into()))),
        op: BinOp::In,
        rhs: Box::new(sp(Expr::Ident("s".into()))),
    });
    match &negate_expr(&e).node {
        Expr::BinOp {
            op: BinOp::NotIn, ..
        } => {}
        other => panic!("expected NotIn, got {other:?}"),
    }
}

#[test]
fn negate_expr_de_morgan_laws() {
    let sp = |e| Spanned::no_span(e);

    // And => Or with negated operands
    let e = sp(Expr::BinOp {
        lhs: Box::new(sp(Expr::Ident("a".into()))),
        op: BinOp::And,
        rhs: Box::new(sp(Expr::Ident("b".into()))),
    });
    match &negate_expr(&e).node {
        Expr::BinOp {
            lhs,
            op: BinOp::Or,
            rhs,
        } => {
            assert!(matches!(
                &lhs.node,
                Expr::UnaryOp {
                    op: UnaryOp::Not,
                    ..
                }
            ));
            assert!(matches!(
                &rhs.node,
                Expr::UnaryOp {
                    op: UnaryOp::Not,
                    ..
                }
            ));
        }
        other => panic!("expected Or, got {other:?}"),
    }

    // Or => And with negated operands
    let e = sp(Expr::BinOp {
        lhs: Box::new(sp(Expr::Ident("a".into()))),
        op: BinOp::Or,
        rhs: Box::new(sp(Expr::Ident("b".into()))),
    });
    match &negate_expr(&e).node {
        Expr::BinOp {
            lhs,
            op: BinOp::And,
            rhs,
        } => {
            assert!(matches!(
                &lhs.node,
                Expr::UnaryOp {
                    op: UnaryOp::Not,
                    ..
                }
            ));
            assert!(matches!(
                &rhs.node,
                Expr::UnaryOp {
                    op: UnaryOp::Not,
                    ..
                }
            ));
        }
        other => panic!("expected And, got {other:?}"),
    }
}

#[test]
fn negate_expr_double_negation_elimination() {
    let sp = |e| Spanned::no_span(e);

    let e = sp(Expr::UnaryOp {
        op: UnaryOp::Not,
        expr: Box::new(sp(Expr::Ident("x".into()))),
    });
    match &negate_expr(&e).node {
        Expr::Ident(name) => assert_eq!(name, "x"),
        other => panic!("expected Ident, got {other:?}"),
    }
}

#[test]
fn negate_expr_bool_literal() {
    let sp = |e| Spanned::no_span(e);

    let e = sp(Expr::Literal(Literal::Bool(true)));
    match &negate_expr(&e).node {
        Expr::Literal(Literal::Bool(false)) => {}
        other => panic!("expected false, got {other:?}"),
    }

    let e = sp(Expr::Literal(Literal::Bool(false)));
    match &negate_expr(&e).node {
        Expr::Literal(Literal::Bool(true)) => {}
        other => panic!("expected true, got {other:?}"),
    }
}
