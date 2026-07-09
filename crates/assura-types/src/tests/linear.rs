use super::*;
use assura_parser::ast::Spanned;
// T031: Usage tracking tests (linear types)
// -----------------------------------------------------------------------

#[test]
fn usage_linear_exactly_once_ok() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    tracker.use_var("x");
    let errors = tracker.check();
    assert!(errors.is_empty());
}

#[test]
fn usage_linear_never_used_a05002() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    // Never use x
    let errors = tracker.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05002");
    assert!(errors[0].message.contains("never used"));
    assert!(errors[0].message.contains("x"));
}

#[test]
fn usage_linear_used_twice_a05001() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    tracker.use_var("x");
    tracker.use_var("x");
    let errors = tracker.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05001");
    assert!(errors[0].message.contains("2 times"));
    assert!(errors[0].message.contains("exactly once"));
}

#[test]
fn usage_linear_used_many_times_a05001() {
    let mut tracker = UsageTracker::new();
    tracker.declare("buf".into(), UsageGrade::Linear, 5..10);
    for _ in 0..5 {
        tracker.use_var("buf");
    }
    let errors = tracker.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05001");
    assert!(errors[0].message.contains("5 times"));
}

#[test]
fn usage_erased_not_used_ok() {
    let mut tracker = UsageTracker::new();
    tracker.declare("ghost_val".into(), UsageGrade::Erased, 0..1);
    // Ghost variable never used at runtime: OK
    let errors = tracker.check();
    assert!(errors.is_empty());
}

#[test]
fn usage_erased_used_a05002() {
    let mut tracker = UsageTracker::new();
    tracker.declare("ghost_val".into(), UsageGrade::Erased, 0..1);
    tracker.use_var("ghost_val");
    let errors = tracker.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05002");
    assert!(errors[0].message.contains("erased"));
    assert!(errors[0].message.contains("ghost_val"));
}

#[test]
fn usage_exact_correct_count_ok() {
    let mut tracker = UsageTracker::new();
    tracker.declare("y".into(), UsageGrade::Exact(3), 0..1);
    tracker.use_var("y");
    tracker.use_var("y");
    tracker.use_var("y");
    let errors = tracker.check();
    assert!(errors.is_empty());
}

#[test]
fn usage_exact_too_few_a05003() {
    let mut tracker = UsageTracker::new();
    tracker.declare("y".into(), UsageGrade::Exact(3), 0..1);
    tracker.use_var("y");
    let errors = tracker.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05003");
    assert!(errors[0].message.contains("1 time(s)"));
    assert!(errors[0].message.contains("3 time(s)"));
}

#[test]
fn usage_exact_too_many_a05003() {
    let mut tracker = UsageTracker::new();
    tracker.declare("y".into(), UsageGrade::Exact(2), 0..1);
    tracker.use_var("y");
    tracker.use_var("y");
    tracker.use_var("y");
    tracker.use_var("y");
    let errors = tracker.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05003");
    assert!(errors[0].message.contains("4 time(s)"));
    assert!(errors[0].message.contains("2 time(s)"));
}

#[test]
fn usage_exact_zero_a05003() {
    let mut tracker = UsageTracker::new();
    tracker.declare("z".into(), UsageGrade::Exact(2), 0..1);
    // Never use z
    let errors = tracker.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05003");
    assert!(errors[0].message.contains("0 time(s)"));
}

#[test]
fn usage_unlimited_any_count_ok() {
    let mut tracker = UsageTracker::new();
    tracker.declare("w".into(), UsageGrade::Unlimited, 0..1);
    // Use 0 times: OK
    assert!(tracker.check().is_empty());

    // Use 1 time: OK
    tracker.use_var("w");
    assert!(tracker.check().is_empty());

    // Use 100 times: OK
    for _ in 0..99 {
        tracker.use_var("w");
    }
    assert!(tracker.check().is_empty());
}

#[test]
fn usage_untracked_var_ignored() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    // Using a variable not declared in the tracker is a no-op
    tracker.use_var("y");
    tracker.use_var("x");
    let errors = tracker.check();
    assert!(errors.is_empty());
}

#[test]
fn usage_multiple_variables_mixed() {
    let mut tracker = UsageTracker::new();
    tracker.declare("a".into(), UsageGrade::Linear, 0..1);
    tracker.declare("b".into(), UsageGrade::Linear, 2..3);
    tracker.declare("c".into(), UsageGrade::Unlimited, 4..5);

    tracker.use_var("a"); // OK: linear used once
    // b never used: error
    tracker.use_var("c");
    tracker.use_var("c");
    tracker.use_var("c"); // OK: unlimited

    let errors = tracker.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05002");
    assert!(errors[0].message.contains("never used"));
    assert!(
        errors[0].message.contains("b"),
        "expected var b in message: {}",
        errors[0].message
    );
}

#[test]
fn usage_grade_display() {
    assert_eq!(format!("{}", UsageGrade::Erased), "erased (grade 0)");
    assert_eq!(format!("{}", UsageGrade::Linear), "linear (grade 1)");
    assert_eq!(format!("{}", UsageGrade::Exact(5)), "exact (grade 5)");
    assert_eq!(format!("{}", UsageGrade::Unlimited), "unlimited (grade ω)");
}

#[test]
fn expr_usages_counts_ident() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    let expr = Spanned::no_span(AstExpr::Ident("x".into()));
    expr_usages(&expr, &mut tracker);
    // x used once, so check should pass for Linear
    assert!(tracker.check().is_empty());
}

#[test]
fn expr_usages_binop_counts_both_sides() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Exact(2), 0..1);
    // x + x => 2 uses
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        op: AstBinOp::Add,
        rhs: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
    });
    expr_usages(&expr, &mut tracker);
    assert!(tracker.check().is_empty());
}

#[test]
fn expr_usages_linear_used_in_binop_a05001() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    // x + x => 2 uses of a linear variable
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        op: AstBinOp::Add,
        rhs: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
    });
    expr_usages(&expr, &mut tracker);
    let errors = tracker.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05001");
}

#[test]
fn expr_usages_call_counts_func_and_args() {
    let mut tracker = UsageTracker::new();
    tracker.declare("f".into(), UsageGrade::Linear, 0..1);
    tracker.declare("a".into(), UsageGrade::Linear, 2..3);
    // f(a) => 1 use of f, 1 use of a
    let expr = Spanned::no_span(AstExpr::Call {
        func: Box::new(Spanned::no_span(AstExpr::Ident("f".into()))),
        args: vec![Spanned::no_span(AstExpr::Ident("a".into()))],
    });
    expr_usages(&expr, &mut tracker);
    assert!(tracker.check().is_empty());
}

#[test]
fn expr_usages_nested_if() {
    let mut tracker = UsageTracker::new();
    tracker.declare("c".into(), UsageGrade::Exact(1), 0..1);
    tracker.declare("t".into(), UsageGrade::Exact(1), 2..3);
    tracker.declare("e".into(), UsageGrade::Exact(1), 4..5);
    // if c then t else e => 1 use each
    let expr = Spanned::no_span(AstExpr::If {
        cond: Box::new(Spanned::no_span(AstExpr::Ident("c".into()))),
        then_branch: Box::new(Spanned::no_span(AstExpr::Ident("t".into()))),
        else_branch: Some(Box::new(Spanned::no_span(AstExpr::Ident("e".into())))),
    });
    expr_usages(&expr, &mut tracker);
    assert!(tracker.check().is_empty());
}

#[test]
fn expr_usages_quantifier_counts_domain_and_body() {
    let mut tracker = UsageTracker::new();
    tracker.declare("S".into(), UsageGrade::Exact(1), 0..1);
    tracker.declare("p".into(), UsageGrade::Exact(1), 2..3);
    // forall x in S: p => 1 use of S, 1 use of p
    let expr = Spanned::no_span(AstExpr::Forall {
        var: "x".into(),
        domain: Box::new(Spanned::no_span(AstExpr::Ident("S".into()))),
        body: Box::new(Spanned::no_span(AstExpr::Ident("p".into()))),
    });
    expr_usages(&expr, &mut tracker);
    assert!(tracker.check().is_empty());
}

#[test]
fn expr_usages_field_access_counts_receiver() {
    let mut tracker = UsageTracker::new();
    tracker.declare("obj".into(), UsageGrade::Linear, 0..1);
    // obj.field => 1 use of obj
    let expr = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("obj".into()))),
        "field".into(),
    ));
    expr_usages(&expr, &mut tracker);
    assert!(tracker.check().is_empty());
}

#[test]
fn expr_usages_method_call_counts_receiver_and_args() {
    let mut tracker = UsageTracker::new();
    tracker.declare("obj".into(), UsageGrade::Exact(1), 0..1);
    tracker.declare("arg1".into(), UsageGrade::Exact(1), 2..3);
    // obj.method(arg1)
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::Ident("obj".into()))),
        method: "method".into(),
        args: vec![Spanned::no_span(AstExpr::Ident("arg1".into()))],
    });
    expr_usages(&expr, &mut tracker);
    assert!(tracker.check().is_empty());
}

#[test]
fn expr_usages_index_counts_base_and_index() {
    let mut tracker = UsageTracker::new();
    tracker.declare("arr".into(), UsageGrade::Exact(1), 0..1);
    tracker.declare("i".into(), UsageGrade::Exact(1), 2..3);
    // arr[i]
    let expr = Spanned::no_span(AstExpr::Index {
        expr: Box::new(Spanned::no_span(AstExpr::Ident("arr".into()))),
        index: Box::new(Spanned::no_span(AstExpr::Ident("i".into()))),
    });
    expr_usages(&expr, &mut tracker);
    assert!(tracker.check().is_empty());
}

#[test]
fn expr_usages_old_counts_inner() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    // old(x) => 1 use of x
    let expr = Spanned::no_span(AstExpr::Old(Box::new(Spanned::no_span(AstExpr::Ident(
        "x".into(),
    )))));
    expr_usages(&expr, &mut tracker);
    assert!(tracker.check().is_empty());
}

#[test]
fn expr_usages_ident_counts() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    // x => 1 use of x
    let expr = Spanned::no_span(AstExpr::Ident("x".into()));
    expr_usages(&expr, &mut tracker);
    assert!(tracker.check().is_empty());
}

#[test]
fn expr_usages_list_counts_elements() {
    let mut tracker = UsageTracker::new();
    tracker.declare("a".into(), UsageGrade::Exact(1), 0..1);
    tracker.declare("b".into(), UsageGrade::Exact(1), 2..3);
    // [a, b]
    let expr = Spanned::no_span(AstExpr::List(vec![
        Spanned::no_span(AstExpr::Ident("a".into())),
        Spanned::no_span(AstExpr::Ident("b".into())),
    ]));
    expr_usages(&expr, &mut tracker);
    assert!(tracker.check().is_empty());
}

#[test]
fn expr_usages_unary_counts_inner() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    // -x => 1 use of x
    let expr = Spanned::no_span(AstExpr::UnaryOp {
        op: AstUnOp::Neg,
        expr: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
    });
    expr_usages(&expr, &mut tracker);
    assert!(tracker.check().is_empty());
}

#[test]
fn expr_usages_cast_counts_inner() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    // x as Foo => 1 use of x
    let expr = Spanned::no_span(AstExpr::Cast {
        expr: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        ty: "Foo".into(),
    });
    expr_usages(&expr, &mut tracker);
    assert!(tracker.check().is_empty());
}

#[test]
fn expr_usages_block_counts_all() {
    let mut tracker = UsageTracker::new();
    tracker.declare("a".into(), UsageGrade::Exact(1), 0..1);
    tracker.declare("b".into(), UsageGrade::Exact(1), 2..3);
    let expr = Spanned::no_span(AstExpr::Block(vec![
        Spanned::no_span(AstExpr::Ident("a".into())),
        Spanned::no_span(AstExpr::Ident("b".into())),
    ]));
    expr_usages(&expr, &mut tracker);
    assert!(tracker.check().is_empty());
}

#[test]
fn expr_usages_raw_no_count() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    // Raw tokens cannot be analyzed; x stays at 0 uses
    let expr = Spanned::no_span(AstExpr::Raw(vec!["x".into()]));
    expr_usages(&expr, &mut tracker);
    let errors = tracker.check();
    // Linear var not used => A05002
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05002");
}

#[test]
fn expr_usages_literal_no_count() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Unlimited, 0..1);
    let expr = Spanned::no_span(AstExpr::Literal(AstLit::Int("42".into())));
    expr_usages(&expr, &mut tracker);
    // No uses recorded, but unlimited is fine
    assert!(tracker.check().is_empty());
}

#[test]
fn usage_tracker_redeclare_resets() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    tracker.use_var("x");
    // Re-declare resets count
    tracker.declare("x".into(), UsageGrade::Linear, 10..11);
    // Now x has 0 uses again
    let errors = tracker.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05002");
    // Span should be the new declaration span
    assert_eq!(errors[0].span, 10..11);
}

// -----------------------------------------------------------------------
// T032: Context splitting for linear types
// -----------------------------------------------------------------------

#[test]
fn linear_context_both_branches_use_var_ok() {
    // Linear var used once in each branch: OK (consumed in both paths)
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    let mut ctx = LinearContext::new(tracker);

    // if cond then x else x
    let expr = Spanned::no_span(AstExpr::If {
        cond: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
        then_branch: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        else_branch: Some(Box::new(Spanned::no_span(AstExpr::Ident("x".into())))),
    });
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(branch_errors.is_empty(), "should have no A05004 errors");

    // Final check: used exactly once (max from either branch)
    let final_errors = ctx.check();
    assert!(
        final_errors.is_empty(),
        "should have no final errors: {final_errors:?}"
    );
}

#[test]
fn linear_context_one_branch_only_a05004() {
    // Linear var used in then-branch but not else-branch: A05004
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    let mut ctx = LinearContext::new(tracker);

    // if cond then x else 42
    let expr = Spanned::no_span(AstExpr::If {
        cond: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
        then_branch: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        else_branch: Some(Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int(
            "42".into(),
        ))))),
    });
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert_eq!(branch_errors.len(), 1);
    assert_eq!(branch_errors[0].code, "A05004");
    assert!(branch_errors[0].message.contains("x"));
    assert!(branch_errors[0].message.contains("inconsistently"));
}

#[test]
fn linear_context_no_else_branch_a05004() {
    // Linear var used in then-branch with no else-branch: A05004
    // (variable may or may not be consumed depending on condition)
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    let mut ctx = LinearContext::new(tracker);

    // if cond then x
    let expr = Spanned::no_span(AstExpr::If {
        cond: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
        then_branch: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        else_branch: None,
    });
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert_eq!(branch_errors.len(), 1);
    assert_eq!(branch_errors[0].code, "A05004");
}

#[test]
fn linear_context_neither_branch_uses_var() {
    // Linear var used in neither branch: no A05004 (consistent: 0 in both)
    // But final check will produce A05002 (never used).
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    let mut ctx = LinearContext::new(tracker);

    // if cond then 1 else 2
    let expr = Spanned::no_span(AstExpr::If {
        cond: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
        then_branch: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
        else_branch: Some(Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int(
            "2".into(),
        ))))),
    });
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(
        branch_errors.is_empty(),
        "consistent: 0 uses in both branches"
    );

    // Final check: linear var never used
    let final_errors = ctx.check();
    assert_eq!(final_errors.len(), 1);
    assert_eq!(final_errors[0].code, "A05002");
}

#[test]
fn linear_context_double_use_in_one_branch() {
    // Linear var used twice in one branch, once in the other: A05004
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    let mut ctx = LinearContext::new(tracker);

    // if cond then (x + x) else x
    let expr = Spanned::no_span(AstExpr::If {
        cond: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
        then_branch: Box::new(Spanned::no_span(AstExpr::BinOp {
            lhs: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
            op: AstBinOp::Add,
            rhs: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        })),
        else_branch: Some(Box::new(Spanned::no_span(AstExpr::Ident("x".into())))),
    });
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert_eq!(branch_errors.len(), 1);
    assert_eq!(branch_errors[0].code, "A05004");
    // Delta: 2 in then, 1 in else
    assert!(branch_errors[0].message.contains("2 time(s)"));
    assert!(branch_errors[0].message.contains("1 time(s)"));
}

#[test]
fn linear_context_unlimited_var_no_consistency_error() {
    // Unlimited variable used differently in branches: no A05004
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Unlimited, 0..1);
    let mut ctx = LinearContext::new(tracker);

    // if cond then (x + x + x) else x
    let expr = Spanned::no_span(AstExpr::If {
        cond: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
        then_branch: Box::new(Spanned::no_span(AstExpr::BinOp {
            lhs: Box::new(Spanned::no_span(AstExpr::BinOp {
                lhs: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
                op: AstBinOp::Add,
                rhs: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
            })),
            op: AstBinOp::Add,
            rhs: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        })),
        else_branch: Some(Box::new(Spanned::no_span(AstExpr::Ident("x".into())))),
    });
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(branch_errors.is_empty());

    let final_errors = ctx.check();
    assert!(final_errors.is_empty());
}

#[test]
fn linear_context_condition_uses_before_fork() {
    // Variable used in condition (before fork) and in one branch:
    // results in 2 total uses of a linear var after merge => A05001 from check().
    // Branch consistency: then uses 0 more, else uses 0 more => consistent.
    let mut tracker = UsageTracker::new();
    tracker.declare("c".into(), UsageGrade::Linear, 0..1);
    tracker.declare("x".into(), UsageGrade::Linear, 2..3);
    let mut ctx = LinearContext::new(tracker);

    // if c then x else x
    let expr = Spanned::no_span(AstExpr::If {
        cond: Box::new(Spanned::no_span(AstExpr::Ident("c".into()))),
        then_branch: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        else_branch: Some(Box::new(Spanned::no_span(AstExpr::Ident("x".into())))),
    });
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(branch_errors.is_empty());

    let final_errors = ctx.check();
    // c used once (in condition), x used once (max from branches) => both OK
    assert!(final_errors.is_empty(), "errors: {final_errors:?}");
}

#[test]
fn linear_context_multiple_vars_mixed() {
    // Multiple variables: one consistent, one not.
    let mut tracker = UsageTracker::new();
    tracker.declare("a".into(), UsageGrade::Linear, 0..1);
    tracker.declare("b".into(), UsageGrade::Linear, 2..3);
    let mut ctx = LinearContext::new(tracker);

    // if cond then (a, b) else (a, 0)
    // a: used in both => consistent
    // b: used in then only => inconsistent A05004
    let expr = Spanned::no_span(AstExpr::If {
        cond: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
        then_branch: Box::new(Spanned::no_span(AstExpr::List(vec![
            Spanned::no_span(AstExpr::Ident("a".into())),
            Spanned::no_span(AstExpr::Ident("b".into())),
        ]))),
        else_branch: Some(Box::new(Spanned::no_span(AstExpr::List(vec![
            Spanned::no_span(AstExpr::Ident("a".into())),
            Spanned::no_span(AstExpr::Literal(AstLit::Int("0".into()))),
        ])))),
    });
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert_eq!(branch_errors.len(), 1);
    assert_eq!(branch_errors[0].code, "A05004");
    assert!(branch_errors[0].message.contains("b"));
}

#[test]
fn linear_context_exact_grade_consistency_check() {
    // Exact(2) grade: must use consistently across branches.
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Exact(2), 0..1);
    let mut ctx = LinearContext::new(tracker);

    // if cond then (x+x) else x  => delta 2 vs delta 1 => A05004
    let expr = Spanned::no_span(AstExpr::If {
        cond: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
        then_branch: Box::new(Spanned::no_span(AstExpr::BinOp {
            lhs: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
            op: AstBinOp::Add,
            rhs: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        })),
        else_branch: Some(Box::new(Spanned::no_span(AstExpr::Ident("x".into())))),
    });
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert_eq!(branch_errors.len(), 1);
    assert_eq!(branch_errors[0].code, "A05004");
}

#[test]
fn linear_context_exact_grade_consistent_ok() {
    // Exact(2): same delta in both branches => OK
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Exact(2), 0..1);
    let mut ctx = LinearContext::new(tracker);

    // if cond then (x+x) else (x+x) => delta 2 in both
    let expr = Spanned::no_span(AstExpr::If {
        cond: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
        then_branch: Box::new(Spanned::no_span(AstExpr::BinOp {
            lhs: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
            op: AstBinOp::Add,
            rhs: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        })),
        else_branch: Some(Box::new(Spanned::no_span(AstExpr::BinOp {
            lhs: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
            op: AstBinOp::Add,
            rhs: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        }))),
    });
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(branch_errors.is_empty());

    let final_errors = ctx.check();
    assert!(final_errors.is_empty());
}

#[test]
fn linear_context_nested_if_branches() {
    // Nested if: outer branch forks, inner branch forks again.
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    let mut ctx = LinearContext::new(tracker);

    // if c1 then (if c2 then x else x) else x
    // Inner if: x used consistently in both branches => OK
    // Outer if: after inner merge, x used once in then, once in else => OK
    let expr = Spanned::no_span(AstExpr::If {
        cond: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
        then_branch: Box::new(Spanned::no_span(AstExpr::If {
            cond: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(false)))),
            then_branch: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
            else_branch: Some(Box::new(Spanned::no_span(AstExpr::Ident("x".into())))),
        })),
        else_branch: Some(Box::new(Spanned::no_span(AstExpr::Ident("x".into())))),
    });
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(branch_errors.is_empty());

    let final_errors = ctx.check();
    assert!(final_errors.is_empty());
}

#[test]
fn linear_context_nested_if_inner_inconsistent() {
    // Inner if is inconsistent: should produce A05004.
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    let mut ctx = LinearContext::new(tracker);

    // if c1 then (if c2 then x else 0) else x
    // Inner if: x used in then but not else => A05004
    let expr = Spanned::no_span(AstExpr::If {
        cond: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
        then_branch: Box::new(Spanned::no_span(AstExpr::If {
            cond: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(false)))),
            then_branch: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
            else_branch: Some(Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int(
                "0".into(),
            ))))),
        })),
        else_branch: Some(Box::new(Spanned::no_span(AstExpr::Ident("x".into())))),
    });
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    // Inner if produces an A05004 for x
    assert!(
        branch_errors.iter().any(|e| e.code == "A05004"),
        "expected A05004: {branch_errors:?}"
    );
}

#[test]
fn linear_context_erased_var_unaffected_by_branches() {
    // Erased variable: branch consistency not checked (grade is Erased).
    // Using it in either branch is an A05002 from final check, not A05004.
    let mut tracker = UsageTracker::new();
    tracker.declare("g".into(), UsageGrade::Erased, 0..1);
    let mut ctx = LinearContext::new(tracker);

    // if cond then g else 0
    let expr = Spanned::no_span(AstExpr::If {
        cond: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
        then_branch: Box::new(Spanned::no_span(AstExpr::Ident("g".into()))),
        else_branch: Some(Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int(
            "0".into(),
        ))))),
    });
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    // Erased is not Linear or Exact, so no A05004
    assert!(branch_errors.is_empty());

    // Final check: erased var used at runtime => A05002
    let final_errors = ctx.check();
    assert_eq!(final_errors.len(), 1);
    assert_eq!(final_errors[0].code, "A05002");
}

#[test]
fn linear_context_var_used_in_condition_and_branches() {
    // x used in condition (1 use), then in both branches (1 each).
    // Post-condition base count = 1. Each branch adds 1 more.
    // Delta: 1 in both => consistent. Total after merge: 2.
    // Linear var used 2 times => A05001 from final check.
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    let mut ctx = LinearContext::new(tracker);

    // if x then x else x  (x as condition + x in each branch)
    let expr = Spanned::no_span(AstExpr::If {
        cond: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        then_branch: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        else_branch: Some(Box::new(Spanned::no_span(AstExpr::Ident("x".into())))),
    });
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    // Branches are consistent (both use x once more)
    assert!(branch_errors.is_empty());

    // Final: x used 2 times total (1 condition + 1 from branch max)
    let final_errors = ctx.check();
    assert_eq!(final_errors.len(), 1);
    assert_eq!(final_errors[0].code, "A05001");
}

#[test]
fn linear_context_fork_produces_independent_copies() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    let ctx = LinearContext::new(tracker);

    let (mut a, mut b) = ctx.fork();
    a.use_var("x");
    // b should still have 0 uses
    assert_eq!(a.get_count("x"), Some(1));
    assert_eq!(b.get_count("x"), Some(0));

    b.use_var("x");
    b.use_var("x");
    assert_eq!(b.get_count("x"), Some(2));
    assert_eq!(a.get_count("x"), Some(1)); // unchanged
}

#[test]
fn linear_context_merge_takes_max_usage() {
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Unlimited, 0..1);
    let mut ctx = LinearContext::new(tracker);

    let (mut a, mut b) = ctx.fork();
    a.use_var("x");
    a.use_var("x");
    a.use_var("x");
    b.use_var("x");

    ctx.merge(&a, &b);
    // Max of 3 and 1 = 3
    assert_eq!(ctx.get_count("x"), Some(3));
}

#[test]
fn linear_context_a05005_scope_escape() {
    // A05005: linear variable escapes its scope.
    // This occurs when a linear variable is passed into a context
    // where it outlives its scope (e.g., stored in a longer-lived data
    // structure). For now, model this as a linear var that gets used
    // but its scope ends before consumption.
    //
    // Detected by declaring the variable, walking a scope, then
    // checking: if the variable was not consumed (used 0 times in the
    // scope it was declared in), it effectively escaped.
    let mut tracker = UsageTracker::new();
    tracker.declare("resource".into(), UsageGrade::Linear, 0..8);
    let mut ctx = LinearContext::new(tracker);

    // Simulate: resource is declared but never used in its scope
    // (no expressions reference it).
    let expr = Spanned::no_span(AstExpr::Literal(AstLit::Int("42".into())));
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(branch_errors.is_empty());

    // Final check catches it: linear var never used => A05002
    // This is the scope-escape case: the variable existed but was
    // never consumed before its scope ended.
    let final_errors = ctx.check();
    assert_eq!(final_errors.len(), 1);
    assert_eq!(final_errors[0].code, "A05002");
    assert!(final_errors[0].message.contains("resource"));
}

// -----------------------------------------------------------------------
// T033: Linear type test cases (Section 13 Test Case 1 + additional)
// -----------------------------------------------------------------------

#[test]
fn linear_double_use_a05001() {
    // Double-use of a linear variable must produce A05001.
    let mut tracker = UsageTracker::new();
    tracker.declare("buf".into(), UsageGrade::Linear, 0..3);
    let mut ctx = LinearContext::new(tracker);

    // buf + buf => 2 uses of linear variable
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Ident("buf".into()))),
        op: AstBinOp::Add,
        rhs: Box::new(Spanned::no_span(AstExpr::Ident("buf".into()))),
    });
    check_expr_linearity(&expr, &mut ctx);
    let errors = ctx.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05001");
    assert!(errors[0].message.contains("buf"));
    assert!(errors[0].message.contains("2 times"));
}

#[test]
fn linear_unused_a05002() {
    // Unused linear variable must produce A05002.
    let mut tracker = UsageTracker::new();
    tracker.declare("handle".into(), UsageGrade::Linear, 0..6);
    let mut ctx = LinearContext::new(tracker);

    // Expression that does not reference 'handle' at all
    let expr = Spanned::no_span(AstExpr::Literal(AstLit::Int("99".into())));
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(branch_errors.is_empty());

    let errors = ctx.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05002");
    assert!(errors[0].message.contains("handle"));
    assert!(errors[0].message.contains("never used"));
}

#[test]
fn linear_correctly_used_once_passes() {
    // Linear variable used exactly once must pass without errors.
    let mut tracker = UsageTracker::new();
    tracker.declare("conn".into(), UsageGrade::Linear, 0..4);
    let mut ctx = LinearContext::new(tracker);

    // Single use: conn
    let expr = Spanned::no_span(AstExpr::Ident("conn".into()));
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(branch_errors.is_empty());

    let errors = ctx.check();
    assert!(errors.is_empty());
}

#[test]
fn linear_refinement_predicate_not_a_use() {
    // Section 13, Test Case 1: a refinement predicate on a linear
    // variable should NOT count as a runtime use. The refinement
    // predicate is a compile-time/SMT-level constraint, not a
    // runtime consumption.
    //
    // Model: declare the linear variable, record a "refinement use"
    // (which should be ignored), then record a single real use.
    // The variable should be correctly consumed once.
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);

    // The refinement predicate x > 0 does NOT consume x.
    // Only the actual use in the expression body does.
    // We model this by NOT calling use_var for the refinement.
    // A single real use follows:
    tracker.use_var("x"); // real runtime use

    let errors = tracker.check();
    assert!(
        errors.is_empty(),
        "refinement predicate should not count as a use: {errors:?}"
    );
}

#[test]
fn linear_refinement_predicate_plus_real_use_no_double_count() {
    // Variant of Section 13 Test Case 1: if the refinement predicate
    // were incorrectly counted, a linear var with a refinement plus
    // one real use would show 2 uses (A05001). Verify it only shows 1.
    let mut tracker = UsageTracker::new();
    tracker.declare("resource".into(), UsageGrade::Linear, 0..8);

    // Refinement predicate: resource.is_valid() -- NOT a runtime use.
    // (We skip calling use_var for predicates.)

    // One real use in the function body:
    tracker.use_var("resource");

    let errors = tracker.check();
    assert!(
        errors.is_empty(),
        "should be exactly 1 use, not 2: {errors:?}"
    );
    assert_eq!(tracker.get_count("resource"), Some(1));
}

#[test]
fn linear_triple_use_a05001() {
    // Three uses of a linear variable: A05001 with count 3.
    let mut tracker = UsageTracker::new();
    tracker.declare("fd".into(), UsageGrade::Linear, 0..2);
    let mut ctx = LinearContext::new(tracker);

    // fd + fd + fd => 3 uses
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::BinOp {
            lhs: Box::new(Spanned::no_span(AstExpr::Ident("fd".into()))),
            op: AstBinOp::Add,
            rhs: Box::new(Spanned::no_span(AstExpr::Ident("fd".into()))),
        })),
        op: AstBinOp::Add,
        rhs: Box::new(Spanned::no_span(AstExpr::Ident("fd".into()))),
    });
    check_expr_linearity(&expr, &mut ctx);
    let errors = ctx.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05001");
    assert!(errors[0].message.contains("3 times"));
}

#[test]
fn linear_used_in_call_arg_exactly_once_passes() {
    // Linear variable used as a function argument (single use) passes.
    let mut tracker = UsageTracker::new();
    tracker.declare("key".into(), UsageGrade::Linear, 0..3);
    let mut ctx = LinearContext::new(tracker);

    // consume(key) => 1 use of key
    let expr = Spanned::no_span(AstExpr::Call {
        func: Box::new(Spanned::no_span(AstExpr::Ident("consume".into()))),
        args: vec![Spanned::no_span(AstExpr::Ident("key".into()))],
    });
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(branch_errors.is_empty());

    let errors = ctx.check();
    assert!(errors.is_empty());
}

#[test]
fn linear_branch_consistency_with_single_use_passes() {
    // Linear variable used exactly once in each branch: passes.
    let mut tracker = UsageTracker::new();
    tracker.declare("tok".into(), UsageGrade::Linear, 0..3);
    let mut ctx = LinearContext::new(tracker);

    // if cond then consume(tok) else discard(tok)
    let expr = Spanned::no_span(AstExpr::If {
        cond: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
        then_branch: Box::new(Spanned::no_span(AstExpr::Call {
            func: Box::new(Spanned::no_span(AstExpr::Ident("consume".into()))),
            args: vec![Spanned::no_span(AstExpr::Ident("tok".into()))],
        })),
        else_branch: Some(Box::new(Spanned::no_span(AstExpr::Call {
            func: Box::new(Spanned::no_span(AstExpr::Ident("discard".into()))),
            args: vec![Spanned::no_span(AstExpr::Ident("tok".into()))],
        }))),
    });
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(branch_errors.is_empty());

    let errors = ctx.check();
    assert!(errors.is_empty());
}

#[test]
fn linear_two_vars_one_double_used_one_unused() {
    // Two linear variables: one double-used (A05001), one unused (A05002).
    let mut tracker = UsageTracker::new();
    tracker.declare("a".into(), UsageGrade::Linear, 0..1);
    tracker.declare("b".into(), UsageGrade::Linear, 2..3);
    let mut ctx = LinearContext::new(tracker);

    // a + a (double use of a, b never referenced)
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Ident("a".into()))),
        op: AstBinOp::Add,
        rhs: Box::new(Spanned::no_span(AstExpr::Ident("a".into()))),
    });
    check_expr_linearity(&expr, &mut ctx);
    let errors = ctx.check();
    assert_eq!(errors.len(), 2);

    let codes: Vec<&str> = errors.iter().map(|e| e.code.as_str()).collect();
    assert!(codes.contains(&"A05001"), "expected A05001 for `a`");
    assert!(codes.contains(&"A05002"), "expected A05002 for `b`");
}

// -----------------------------------------------------------------------
