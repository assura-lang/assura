use super::*;
use assura_ast::Spanned;
use assura_ast::*;

fn mk_clause(kind: ClauseKind, body: SpExpr) -> Clause {
    Clause {
        kind,
        body,
        effect_variables: vec![],
    }
}

// ---- extract_state_comparison ----

#[test]
fn state_comparison_match() {
    // self.state == Open
    let body = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Field(
            Box::new(Spanned::no_span(Expr::Ident("self".into()))),
            "state".into(),
        ))),
        op: BinOp::Eq,
        rhs: Box::new(Spanned::no_span(Expr::Ident("Open".into()))),
    });
    assert_eq!(extract_state_comparison(&body), Some("Open".into()));
}

#[test]
fn state_comparison_not_self() {
    let body = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Field(
            Box::new(Spanned::no_span(Expr::Ident("other".into()))),
            "state".into(),
        ))),
        op: BinOp::Eq,
        rhs: Box::new(Spanned::no_span(Expr::Ident("Open".into()))),
    });
    assert_eq!(extract_state_comparison(&body), None);
}

#[test]
fn state_comparison_not_eq() {
    let body = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Field(
            Box::new(Spanned::no_span(Expr::Ident("self".into()))),
            "state".into(),
        ))),
        op: BinOp::Neq,
        rhs: Box::new(Spanned::no_span(Expr::Ident("Open".into()))),
    });
    assert_eq!(extract_state_comparison(&body), None);
}

// ---- collect_service_states ----

#[test]
fn collect_states_present() {
    let s = ServiceDecl {
        name: "MyService".into(),
        items: vec![ServiceItem::States(vec![
            "Init".into(),
            "Running".into(),
            "Done".into(),
        ])],
    };
    assert_eq!(collect_service_states(&s), vec!["Init", "Running", "Done"]);
}

#[test]
fn collect_states_none() {
    let s = ServiceDecl {
        name: "Simple".into(),
        items: vec![],
    };
    assert!(collect_service_states(&s).is_empty());
}

// ---- method_pre_state ----

#[test]
fn pre_state_found() {
    let clauses = vec![mk_clause(
        ClauseKind::Requires,
        Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Field(
                Box::new(Spanned::no_span(Expr::Ident("self".into()))),
                "state".into(),
            ))),
            op: BinOp::Eq,
            rhs: Box::new(Spanned::no_span(Expr::Ident("Init".into()))),
        }),
    )];
    assert_eq!(method_pre_state(&clauses), Some("Init".into()));
}

#[test]
fn pre_state_not_found() {
    let clauses = vec![mk_clause(
        ClauseKind::Requires,
        Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            op: BinOp::Gt,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        }),
    )];
    assert_eq!(method_pre_state(&clauses), None);
}

// ---- generate_service_method ----

#[test]
fn service_method_operation_mut_self() {
    let mut code = String::new();
    generate_service_method(&mut code, "process", &[], true, false, None);
    assert!(code.contains("&mut self"), "operation uses &mut self");
    assert!(code.contains("pub fn process"));
}

#[test]
fn service_method_query_ref_self() {
    let mut code = String::new();
    generate_service_method(&mut code, "get_value", &[], false, false, None);
    assert!(code.contains("&self"), "query uses &self");
    assert!(code.contains("pub fn get_value"));
}

#[test]
fn service_method_with_invariant_check() {
    let mut code = String::new();
    generate_service_method(&mut code, "do_it", &[], true, true, None);
    assert!(
        code.contains("self.check_invariant()"),
        "invariant check on entry/exit"
    );
}

#[test]
fn service_method_with_state_guard() {
    let clauses = vec![mk_clause(
        ClauseKind::Requires,
        Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Field(
                Box::new(Spanned::no_span(Expr::Ident("self".into()))),
                "state".into(),
            ))),
            op: BinOp::Eq,
            rhs: Box::new(Spanned::no_span(Expr::Ident("Ready".into()))),
        }),
    )];
    let mut code = String::new();
    generate_service_method(&mut code, "start", &clauses, true, false, None);
    assert!(
        code.contains("State::Ready"),
        "state guard should be in code"
    );
}

#[test]
fn service_method_with_state_transition() {
    let clauses = vec![mk_clause(
        ClauseKind::Ensures,
        Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Field(
                Box::new(Spanned::no_span(Expr::Ident("self".into()))),
                "state".into(),
            ))),
            op: BinOp::Eq,
            rhs: Box::new(Spanned::no_span(Expr::Ident("Running".into()))),
        }),
    )];
    let mut code = String::new();
    generate_service_method(&mut code, "start", &clauses, true, false, None);
    assert!(
        code.contains("State::Running"),
        "state transition in output"
    );
}

// ---- generate_service (stateless) ----

#[test]
fn service_stateless_struct_and_impl() {
    let s = ServiceDecl {
        name: "Counter".into(),
        items: vec![ServiceItem::Operation {
            name: "increment".into(),
            clauses: vec![],
        }],
    };
    let mut code = String::new();
    generate_service(&s, &mut code, None);
    assert!(code.contains("pub mod counter"));
    assert!(code.contains("pub struct Counter"));
    assert!(code.contains("pub fn new()"));
    assert!(code.contains("pub fn increment"));
}

// ---- generate_service (typestate) ----

#[test]
fn service_typestate_has_marker_structs() {
    let s = ServiceDecl {
        name: "Conn".into(),
        items: vec![
            ServiceItem::States(vec!["Closed".into(), "Open".into()]),
            ServiceItem::Operation {
                name: "open".into(),
                clauses: vec![
                    mk_clause(
                        ClauseKind::Requires,
                        Spanned::no_span(Expr::BinOp {
                            lhs: Box::new(Spanned::no_span(Expr::Field(
                                Box::new(Spanned::no_span(Expr::Ident("self".into()))),
                                "state".into(),
                            ))),
                            op: BinOp::Eq,
                            rhs: Box::new(Spanned::no_span(Expr::Ident("Closed".into()))),
                        }),
                    ),
                    mk_clause(
                        ClauseKind::Ensures,
                        Spanned::no_span(Expr::BinOp {
                            lhs: Box::new(Spanned::no_span(Expr::Field(
                                Box::new(Spanned::no_span(Expr::Ident("self".into()))),
                                "state".into(),
                            ))),
                            op: BinOp::Eq,
                            rhs: Box::new(Spanned::no_span(Expr::Ident("Open".into()))),
                        }),
                    ),
                ],
            },
        ],
    };
    let mut code = String::new();
    generate_service(&s, &mut code, None);
    assert!(code.contains("pub struct Closed;"), "Closed marker");
    assert!(code.contains("pub struct Open;"), "Open marker");
    assert!(code.contains("PhantomData"), "generic state param");
    assert!(code.contains("impl Conn<Closed>"), "initial state impl");
    assert!(code.contains("fn new()"), "new() on initial state");
    assert!(code.contains("-> Conn<Open>"), "state transition return");
}

// ---- generate_interface_trait ----

#[test]
fn interface_simple_method() {
    let clauses = vec![mk_clause(
        ClauseKind::Other("method".into()),
        Spanned::no_span(Expr::Ident("do_something".into())),
    )];
    let mut code = String::new();
    generate_interface_trait("Doable", &clauses, &mut code);
    assert!(code.contains("pub trait Doable"));
    assert!(code.contains("fn do_something(&self);"));
}

#[test]
fn interface_with_extends() {
    let clauses = vec![
        mk_clause(
            ClauseKind::Other("extends".into()),
            Spanned::no_span(Expr::Ident("Base".into())),
        ),
        mk_clause(
            ClauseKind::Other("method".into()),
            Spanned::no_span(Expr::Ident("extra".into())),
        ),
    ];
    let mut code = String::new();
    generate_interface_trait("Extended", &clauses, &mut code);
    assert!(
        code.contains("pub trait Extended: Base"),
        "supertrait bound"
    );
}

#[test]
fn interface_invariant_becomes_provided_method() {
    let clauses = vec![mk_clause(
        ClauseKind::Invariant,
        Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            op: BinOp::Gt,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        }),
    )];
    let mut code = String::new();
    generate_interface_trait("Positive", &clauses, &mut code);
    assert!(code.contains("fn check_invariant(&self)"));
    assert!(code.contains("debug_assert!"));
}

// ---- generate_trait_method ----

#[test]
fn trait_method_ident() {
    let mut code = String::new();
    let body = Spanned::no_span(Expr::Ident("compute".into()));
    generate_trait_method(&body, &mut code);
    assert!(code.contains("fn compute(&self);"));
}

#[test]
fn trait_method_call_with_args() {
    let body = Spanned::no_span(Expr::Call {
        func: Box::new(Spanned::no_span(Expr::Ident("process".into()))),
        args: vec![
            Spanned::no_span(Expr::Ident("Int".into())),
            Spanned::no_span(Expr::Ident("Bool".into())),
        ],
    });
    let mut code = String::new();
    generate_trait_method(&body, &mut code);
    assert!(code.contains("fn process(&self, arg0: i64, arg1: bool)"));
}

#[test]
fn trait_method_unsupported_expr() {
    let body = Spanned::no_span(Expr::Literal(Literal::Int("42".into())));
    let mut code = String::new();
    generate_trait_method(&body, &mut code);
    assert!(code.contains("compile_error!"));
}
