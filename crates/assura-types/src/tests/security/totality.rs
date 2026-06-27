use super::*;

// --- T053 test helpers ---

fn make_fn_def(name: &str, params: Vec<(&str, &[&str])>, clauses: Vec<AstClause>) -> AstFnDef {
    AstFnDef {
        name: name.into(),
        is_ghost: false,
        is_lemma: false,
        params: params
            .into_iter()
            .map(|(n, ty)| {
                let tokens: Vec<String> = ty.iter().map(|s| s.to_string()).collect();
                AstParam {
                    name: n.into(),
                    ty: assura_parser::ast::try_parse_type_tokens(&tokens),
                }
            })
            .collect(),
        return_ty: assura_parser::ast::try_parse_type_tokens(&["Int".to_string()]),
        clauses,
    }
}

fn decreases_clause(body: SpExpr) -> AstClause {
    AstClause {
        kind: ClauseKind::Other("decreases".into()),
        body,
        effect_variables: vec![],
    }
}

fn requires_clause(body: SpExpr) -> AstClause {
    AstClause {
        kind: ClauseKind::Requires,
        body,
        effect_variables: vec![],
    }
}

fn partial_clause() -> AstClause {
    AstClause {
        kind: ClauseKind::Other("partial".into()),
        body: Spanned::no_span(AstExpr::Literal(AstLit::Bool(true))),
        effect_variables: vec![],
    }
}

fn ensures_with_recursive_call(fn_name: &str, args: Vec<SpExpr>) -> AstClause {
    AstClause {
        kind: ClauseKind::Ensures,
        body: Spanned::no_span(AstExpr::Call {
            func: Box::new(Spanned::no_span(AstExpr::Ident(fn_name.into()))),
            args,
        }),
        effect_variables: vec![],
    }
}

#[test]
fn totality_non_recursive_trivially_total() {
    // Non-recursive function passes without decreases
    let fn_def = make_fn_def("add", vec![("a", &["Int"]), ("b", &["Int"])], vec![]);
    let checker = TotalityChecker::new();
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..10));
    assert!(
        errors.is_empty(),
        "non-recursive function should be trivially total"
    );
}

#[test]
fn totality_recursive_with_valid_decreases() {
    // factorial(n) with decreases n, recursive call factorial(n - 1)
    let fn_def = make_fn_def(
        "factorial",
        vec![("n", &["Nat"])],
        vec![
            decreases_clause(Spanned::no_span(AstExpr::Ident("n".into()))),
            ensures_with_recursive_call(
                "factorial",
                vec![Spanned::no_span(AstExpr::BinOp {
                    lhs: Box::new(Spanned::no_span(AstExpr::Ident("n".into()))),
                    op: AstBinOp::Sub,
                    rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
                })],
            ),
        ],
    );
    let checker = TotalityChecker::new();
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..20));
    assert!(
        errors.is_empty(),
        "valid decreasing measure should pass: {errors:?}"
    );
}

#[test]
fn totality_recursive_without_decreases_a09001() {
    // Recursive function without decreases clause -> A09001
    let fn_def = make_fn_def(
        "loop_forever",
        vec![("n", &["Int"])],
        vec![ensures_with_recursive_call(
            "loop_forever",
            vec![Spanned::no_span(AstExpr::Ident("n".into()))],
        )],
    );
    let checker = TotalityChecker::new();
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..10));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A09001");
}

#[test]
fn totality_non_decreasing_measure_deferred_to_smt() {
    // Recursive call with same argument (not decreasing) is now deferred to SMT
    // instead of immediately producing A09002. The SMT solver will find that
    // n < n is unsatisfiable and report the error.
    let fn_def = make_fn_def(
        "spin",
        vec![("n", &["Nat"])],
        vec![
            decreases_clause(Spanned::no_span(AstExpr::Ident("n".into()))),
            ensures_with_recursive_call("spin", vec![Spanned::no_span(AstExpr::Ident("n".into()))]),
        ],
    );
    let checker = TotalityChecker::new();
    let (errors, pending) = checker.check_function_totality(&fn_def, &(0..10));
    // No syntactic error; the check is deferred to SMT
    assert!(
        errors.is_empty(),
        "non-decreasing measure should be deferred to SMT, not produce syntactic error: {errors:?}"
    );
    assert!(
        !pending.is_empty(),
        "non-decreasing measure should produce a pending SMT check"
    );
    // The pending check should reference the spin function
    assert_eq!(pending[0].fn_name, "spin");
}

#[test]
fn totality_measure_not_well_founded_a09003() {
    // decreases n but no requires n >= 0 and param type is Int, not Nat
    let fn_def = make_fn_def(
        "bad_rec",
        vec![("n", &["Int"])],
        vec![
            decreases_clause(Spanned::no_span(AstExpr::Ident("n".into()))),
            ensures_with_recursive_call(
                "bad_rec",
                vec![Spanned::no_span(AstExpr::BinOp {
                    lhs: Box::new(Spanned::no_span(AstExpr::Ident("n".into()))),
                    op: AstBinOp::Sub,
                    rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
                })],
            ),
        ],
    );
    let checker = TotalityChecker::new();
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..10));
    assert!(
        errors.iter().any(|e| e.code == "A09003"),
        "missing well-foundedness should produce A09003: {errors:?}"
    );
}

#[test]
fn totality_well_founded_with_requires_clause() {
    // decreases n with requires n >= 0 should NOT produce A09003
    let fn_def = make_fn_def(
        "count_down",
        vec![("n", &["Int"])],
        vec![
            requires_clause(Spanned::no_span(AstExpr::BinOp {
                lhs: Box::new(Spanned::no_span(AstExpr::Ident("n".into()))),
                op: AstBinOp::Gte,
                rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("0".into())))),
            })),
            decreases_clause(Spanned::no_span(AstExpr::Ident("n".into()))),
            ensures_with_recursive_call(
                "count_down",
                vec![Spanned::no_span(AstExpr::BinOp {
                    lhs: Box::new(Spanned::no_span(AstExpr::Ident("n".into()))),
                    op: AstBinOp::Sub,
                    rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
                })],
            ),
        ],
    );
    let checker = TotalityChecker::new();
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..20));
    assert!(
        !errors.iter().any(|e| e.code == "A09003"),
        "requires n >= 0 should establish well-foundedness: {errors:?}"
    );
}

#[test]
fn totality_partial_escape_hatch() {
    // Partial function skips termination checking
    let fn_def = make_fn_def(
        "diverge",
        vec![("n", &["Int"])],
        vec![
            partial_clause(),
            ensures_with_recursive_call(
                "diverge",
                vec![Spanned::no_span(AstExpr::Ident("n".into()))],
            ),
        ],
    );
    let checker = TotalityChecker::new();
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..10));
    assert!(
        errors.is_empty(),
        "partial function should skip totality check"
    );
}

#[test]
fn totality_partial_via_register() {
    // Partial registered via mark_partial
    let fn_def = make_fn_def(
        "diverge2",
        vec![("n", &["Int"])],
        vec![ensures_with_recursive_call(
            "diverge2",
            vec![Spanned::no_span(AstExpr::Ident("n".into()))],
        )],
    );
    let mut checker = TotalityChecker::new();
    checker.mark_partial("diverge2".into());
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..10));
    assert!(errors.is_empty(), "registered partial should skip check");
}

#[test]
fn totality_lexicographic_measures() {
    // Ackermann-like: decreases (m, n) with call (m - 1, n)
    let fn_def = make_fn_def(
        "ack",
        vec![("m", &["Nat"]), ("n", &["Nat"])],
        vec![
            decreases_clause(Spanned::no_span(AstExpr::Ident("m".into()))),
            decreases_clause(Spanned::no_span(AstExpr::Ident("n".into()))),
            ensures_with_recursive_call(
                "ack",
                vec![
                    Spanned::no_span(AstExpr::BinOp {
                        lhs: Box::new(Spanned::no_span(AstExpr::Ident("m".into()))),
                        op: AstBinOp::Sub,
                        rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
                    }),
                    Spanned::no_span(AstExpr::Ident("n".into())),
                ],
            ),
        ],
    );
    let checker = TotalityChecker::new();
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..20));
    assert!(
        errors.is_empty(),
        "lexicographic decrease in first component should pass: {errors:?}"
    );
}

#[test]
fn totality_mutual_recursion_no_decreases_a09004() {
    // Two functions calling each other with no decreases -> A09004
    let fn_a = make_fn_def(
        "even",
        vec![("n", &["Nat"])],
        vec![ensures_with_recursive_call(
            "odd",
            vec![Spanned::no_span(AstExpr::BinOp {
                lhs: Box::new(Spanned::no_span(AstExpr::Ident("n".into()))),
                op: AstBinOp::Sub,
                rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
            })],
        )],
    );
    let fn_b = make_fn_def(
        "odd",
        vec![("n", &["Nat"])],
        vec![ensures_with_recursive_call(
            "even",
            vec![Spanned::no_span(AstExpr::BinOp {
                lhs: Box::new(Spanned::no_span(AstExpr::Ident("n".into()))),
                op: AstBinOp::Sub,
                rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
            })],
        )],
    );

    let checker = TotalityChecker::new();
    let span_a = 0..10;
    let span_b = 10..20;
    let fn_defs: Vec<(&AstFnDef, &Range<usize>)> = vec![(&fn_a, &span_a), (&fn_b, &span_b)];
    let errors = checker.check_mutual_recursion(&fn_defs);
    assert!(
        errors.iter().any(|e| e.code == "A09004"),
        "mutual recursion without decreases should produce A09004: {errors:?}"
    );
}

#[test]
fn totality_mutual_recursion_with_decreases_passes() {
    // Two functions calling each other, one has decreases -> passes
    let fn_a = make_fn_def(
        "even2",
        vec![("n", &["Nat"])],
        vec![
            decreases_clause(Spanned::no_span(AstExpr::Ident("n".into()))),
            ensures_with_recursive_call(
                "odd2",
                vec![Spanned::no_span(AstExpr::BinOp {
                    lhs: Box::new(Spanned::no_span(AstExpr::Ident("n".into()))),
                    op: AstBinOp::Sub,
                    rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
                })],
            ),
        ],
    );
    let fn_b = make_fn_def(
        "odd2",
        vec![("n", &["Nat"])],
        vec![ensures_with_recursive_call(
            "even2",
            vec![Spanned::no_span(AstExpr::BinOp {
                lhs: Box::new(Spanned::no_span(AstExpr::Ident("n".into()))),
                op: AstBinOp::Sub,
                rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
            })],
        )],
    );

    let checker = TotalityChecker::new();
    let span_a = 0..10;
    let span_b = 10..20;
    let fn_defs: Vec<(&AstFnDef, &Range<usize>)> = vec![(&fn_a, &span_a), (&fn_b, &span_b)];
    let errors = checker.check_mutual_recursion(&fn_defs);
    assert!(
        errors.is_empty(),
        "mutual recursion with decreases should pass: {errors:?}"
    );
}

#[test]
fn totality_structural_recursion_on_list() {
    // list_len(xs) with decreases xs, recursive call list_len(xs.tail)
    let fn_def = make_fn_def(
        "list_len",
        vec![("xs", &["List"])],
        vec![
            decreases_clause(Spanned::no_span(AstExpr::Ident("xs".into()))),
            ensures_with_recursive_call(
                "list_len",
                vec![Spanned::no_span(AstExpr::Field(
                    Box::new(Spanned::no_span(AstExpr::Ident("xs".into()))),
                    "tail".into(),
                ))],
            ),
        ],
    );
    let checker = TotalityChecker::new();
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..20));
    assert!(
        errors.is_empty(),
        "structural recursion on .tail should pass: {errors:?}"
    );
}

#[test]
fn totality_structural_recursion_on_tree() {
    // tree_depth(node) with decreases node, calls tree_depth(node.left)
    let fn_def = make_fn_def(
        "tree_depth",
        vec![("node", &["Tree"])],
        vec![
            decreases_clause(Spanned::no_span(AstExpr::Ident("node".into()))),
            ensures_with_recursive_call(
                "tree_depth",
                vec![Spanned::no_span(AstExpr::Field(
                    Box::new(Spanned::no_span(AstExpr::Ident("node".into()))),
                    "left".into(),
                ))],
            ),
        ],
    );
    let checker = TotalityChecker::new();
    let (errors, _pending) = checker.check_function_totality(&fn_def, &(0..20));
    assert!(
        errors.is_empty(),
        "structural recursion on .left should pass: {errors:?}"
    );
}

#[test]
fn totality_extract_no_decreases() {
    let fn_def = make_fn_def("f", vec![], vec![]);
    let checker = TotalityChecker::new();
    assert!(checker.extract_decreases_measure(&fn_def).is_none());
}

#[test]
fn totality_extract_single_decreases() {
    let fn_def = make_fn_def(
        "f",
        vec![("n", &["Nat"])],
        vec![decreases_clause(Spanned::no_span(AstExpr::Ident(
            "n".into(),
        )))],
    );
    let checker = TotalityChecker::new();
    let measure = checker.extract_decreases_measure(&fn_def);
    assert!(
        matches!(measure, Some(DecreasesMeasure::Natural(_))),
        "single decreases should yield Natural"
    );
}

#[test]
fn totality_extract_lexicographic_decreases() {
    let fn_def = make_fn_def(
        "f",
        vec![("m", &["Nat"]), ("n", &["Nat"])],
        vec![
            decreases_clause(Spanned::no_span(AstExpr::Ident("m".into()))),
            decreases_clause(Spanned::no_span(AstExpr::Ident("n".into()))),
        ],
    );
    let checker = TotalityChecker::new();
    let measure = checker.extract_decreases_measure(&fn_def);
    assert!(
        matches!(measure, Some(DecreasesMeasure::Lexicographic(ref v)) if v.len() == 2),
        "two decreases should yield Lexicographic(2)"
    );
}

#[test]
fn totality_checker_debug() {
    let checker = TotalityChecker::new();
    let dbg = format!("{checker:?}");
    assert!(dbg.contains("TotalityChecker"));
}

#[test]
fn totality_checker_default() {
    let checker = TotalityChecker::default();
    assert!(!checker.is_partial(&make_fn_def("f", vec![], vec![])));
}

// -----------------------------------------------------------------------
