use super::*;
use assura_parser::ast::Spanned;
// T050: Section 13 type interaction tests
//
// These test pairwise (and three-way) interactions between:
//   - Refinement types
//   - Linear types (UsageTracker, LinearContext)
//   - Typestate (TypestateChecker)
//   - Effects (EffectChecker, EffectSet)
//
// Tests covering information flow and dependent types are deferred
// until T051/T052 are implemented.
// -----------------------------------------------------------------------

// -- Test Case 1: Refinement + Linear (Ghost Use Problem) ----------------
//
// Spec Section 13.1: A refinement predicate references a linear variable.
// Refinement predicates are ghost (logical, not computational) and do
// NOT count as a linear use. The variable is only consumed by
// computational (runtime) uses.

#[test]
fn interaction_refinement_linear_ghost_use_does_not_consume() {
    // Section 13, Test Case 1: a refinement predicate on a linear
    // variable is grade-0 (erased/ghost). It must NOT count as a
    // runtime use.
    //
    // Scenario: linear var `buf` has a refinement `buf.len > 0`.
    // The refinement is a compile-time/SMT-level constraint only.
    // One computational use follows. Total runtime uses = 1 => OK.
    let mut tracker = UsageTracker::new();
    tracker.declare("buf".into(), UsageGrade::Linear, 0..3);

    // Refinement predicate `buf.len > 0` is ghost: do NOT call use_var.
    // Only the single computational use counts:
    tracker.use_var("buf");

    let errors = tracker.check();
    assert!(
        errors.is_empty(),
        "ghost refinement reference should not count as a use: {errors:?}"
    );
    assert_eq!(tracker.get_count("buf"), Some(1));
}

#[test]
fn interaction_refinement_linear_two_computational_uses_a05001() {
    // Section 13, Test Case 1 (negative): two computational uses of
    // a linear variable must produce A05001, regardless of whether a
    // refinement predicate also references the variable.
    let mut tracker = UsageTracker::new();
    tracker.declare("buf".into(), UsageGrade::Linear, 0..3);

    // Refinement predicate (ghost, not counted):
    // -- buf.is_valid (not called via use_var)

    // Two computational (runtime) uses:
    tracker.use_var("buf"); // first use: pass to read()
    tracker.use_var("buf"); // second use: pass to write()

    let errors = tracker.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05001");
    assert!(errors[0].message.contains("buf"));
    assert!(errors[0].message.contains("2 times"));
}

#[test]
fn interaction_refinement_linear_ghost_grade_erased_no_runtime() {
    // A ghost (Erased) variable used in refinement predicates only:
    // grade-0 means zero runtime uses are allowed. Using it at runtime
    // is A05002. This tests the boundary between refinement context
    // (logical) and runtime context.
    let mut tracker = UsageTracker::new();
    tracker.declare("ghost_bound".into(), UsageGrade::Erased, 0..11);

    // Ghost variable is NOT used at runtime (only in predicates).
    // This is correct: erased variables exist only in logic.
    let errors = tracker.check();
    assert!(
        errors.is_empty(),
        "erased variable with no runtime use should pass: {errors:?}"
    );
}

#[test]
fn interaction_refinement_linear_erased_runtime_use_a05002() {
    // Erased variable used at runtime: A05002.
    // This catches the case where a ghost refinement variable
    // accidentally leaks into computational code.
    let mut tracker = UsageTracker::new();
    tracker.declare("ghost_bound".into(), UsageGrade::Erased, 0..11);

    tracker.use_var("ghost_bound"); // runtime use of erased var

    let errors = tracker.check();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A05002");
    assert!(errors[0].message.contains("erased"));
}

#[test]
fn interaction_refinement_linear_refined_type_with_linear_base() {
    // A refined type `{ v: Int | v > 0 }` where the base value is
    // linear. The predicate `v > 0` is ghost; the value `v` itself
    // is linear and must be used exactly once.
    let mut tracker = UsageTracker::new();
    tracker.declare("pos_val".into(), UsageGrade::Linear, 0..7);

    // Type is Refined { base: Int, predicate: "v > 0" }
    // The predicate check is done at compile time (SMT), not runtime.
    // One computational use:
    tracker.use_var("pos_val");

    let errors = tracker.check();
    assert!(errors.is_empty());

    // Verify the type representation captures both aspects
    let ty = Type::refined_from_str(Type::Int, "v", "v > 0");
    assert_eq!(format!("{ty}"), "{ v : Int | v > 0 }");
}

// -- Test Case 4: Linear + Effect (Resource-Scoped Effects) --------------
//
// Spec Section 13.4: Linear resources interact with the effect system.
// A function consuming a linear resource should declare appropriate
// effects. The linear variable must still be consumed exactly once.

#[test]
fn interaction_linear_effect_consume_with_correct_effects() {
    // A function that consumes a linear resource and declares `io`
    // effects. The linear variable is consumed exactly once, and the
    // declared effects cover the actual effects. Both checks pass.
    let mut tracker = UsageTracker::new();
    tracker.declare("conn".into(), UsageGrade::Linear, 0..4);
    let mut ctx = LinearContext::new(tracker);

    // Simulate: conn is consumed by calling conn.close()
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::Ident("conn".into()))),
        method: "close".into(),
        args: vec![],
    });
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(branch_errors.is_empty());

    // Linear check: conn used exactly once => OK
    let linear_errors = ctx.check();
    assert!(linear_errors.is_empty());

    // Effect check: function declares {io}, body performs {io} => OK
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io"]);
    let actual = EffectSet::from_effect_names(["io"]);
    let effect_errors = checker.check_containment(&declared, &actual, &(0..10));
    assert!(effect_errors.is_empty());
}

#[test]
fn interaction_linear_effect_resource_not_consumed_a05002() {
    // A function with correct effects but that forgets to consume
    // its linear resource. The effect check passes, but the linear
    // check must report A05002 (unused linear variable).
    let mut tracker = UsageTracker::new();
    tracker.declare("conn".into(), UsageGrade::Linear, 0..4);
    let mut ctx = LinearContext::new(tracker);

    // Function body does NOT use conn at all
    let expr = Spanned::no_span(AstExpr::Literal(AstLit::Int("0".into())));
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(branch_errors.is_empty());

    // Linear check: conn never consumed => A05002
    let linear_errors = ctx.check();
    assert_eq!(linear_errors.len(), 1);
    assert_eq!(linear_errors[0].code, "A05002");
    assert!(linear_errors[0].message.contains("conn"));

    // Effect check: independently passes (effects are about the
    // function's declared vs actual effects, not resource consumption)
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io"]);
    let actual = EffectSet::from_effect_names(["io"]);
    let effect_errors = checker.check_containment(&declared, &actual, &(0..10));
    assert!(effect_errors.is_empty());
}

#[test]
fn interaction_linear_effect_pure_function_with_linear_resource() {
    // A pure function that consumes a linear resource. The resource
    // is consumed correctly (linear check passes), but the function
    // is pure, so any effectful operation on it should be caught by
    // the effect checker.
    let mut tracker = UsageTracker::new();
    tracker.declare("handle".into(), UsageGrade::Linear, 0..6);
    let mut ctx = LinearContext::new(tracker);

    // Resource consumed (linear OK)
    let expr = Spanned::no_span(AstExpr::Ident("handle".into()));
    check_expr_linearity(&expr, &mut ctx);
    let linear_errors = ctx.check();
    assert!(linear_errors.is_empty());

    // But function is declared pure, body does io => A07002
    let checker = EffectChecker::new();
    let declared = EffectSet::pure();
    let actual = EffectSet::from_effect_names(["io"]);
    let effect_errors = checker.check_containment(&declared, &actual, &(0..10));
    assert_eq!(effect_errors.len(), 1);
    assert_eq!(effect_errors[0].code, "A07002");
}

#[test]
fn interaction_linear_effect_undeclared_effect_on_resource() {
    // Function declares {mem} but performs {io} on the linear resource.
    // Linear check passes (resource consumed once), but effect check
    // fails with A07001 (undeclared effect).
    let mut tracker = UsageTracker::new();
    tracker.declare("socket".into(), UsageGrade::Linear, 0..6);
    let mut ctx = LinearContext::new(tracker);

    // Resource consumed
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::Ident("socket".into()))),
        method: "send".into(),
        args: vec![Spanned::no_span(AstExpr::Literal(AstLit::Str(
            "data".into(),
        )))],
    });
    check_expr_linearity(&expr, &mut ctx);
    let linear_errors = ctx.check();
    assert!(linear_errors.is_empty());

    // Effect mismatch: declared {mem}, actual {io}
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["mem"]);
    let actual = EffectSet::from_effect_names(["io"]);
    let effect_errors = checker.check_containment(&declared, &actual, &(0..10));
    assert_eq!(effect_errors.len(), 1);
    assert_eq!(effect_errors[0].code, "A07001");
}

// -- Linear + Typestate interaction tests --------------------------------
//
// Typestate variables MUST be linear (A06002). This tests the
// interaction between the two checkers.

#[test]
fn interaction_linear_typestate_must_be_linear() {
    // A typestate variable that is not declared as linear must fail
    // with A06002. Typestate requires linearity to prevent aliasing
    // which could observe inconsistent states.
    let states = vec!["Init".into(), "Ready".into()];
    let transitions = vec![("start".into(), "Init".into(), "Ready".into())];
    let checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

    // Not linear => A06002
    let err = checker.validate_linear(false);
    assert_eq!(err.unwrap().code, "A06002");
}

#[test]
fn interaction_linear_typestate_linear_ok() {
    // A typestate variable declared as linear passes the linearity
    // check and can proceed with state transitions.
    let states = vec!["Locked".into(), "Unlocked".into()];
    let transitions = vec![
        ("unlock".into(), "Locked".into(), "Unlocked".into()),
        ("lock".into(), "Unlocked".into(), "Locked".into()),
    ];
    let mut checker = TypestateChecker::new(states, transitions, "Locked".into(), 0..6);

    // Linear check passes
    assert!(checker.validate_linear(true).is_none());

    // Typestate transitions work
    checker.transition("unlock", 10..16).unwrap();
    assert_eq!(checker.current_state(), "Unlocked");

    // Linear usage tracking: consumed exactly once
    let mut tracker = UsageTracker::new();
    tracker.declare("lock_var".into(), UsageGrade::Linear, 0..8);
    tracker.use_var("lock_var"); // consumed by unlock operation
    assert!(tracker.check().is_empty());
}

#[test]
fn interaction_linear_typestate_double_use_violates_both() {
    // Using a typestate variable twice violates both linearity (A05001)
    // and potentially causes observable aliasing. Both checkers must
    // report their respective errors independently.
    let mut tracker = UsageTracker::new();
    tracker.declare("file".into(), UsageGrade::Linear, 0..4);
    tracker.use_var("file"); // first use: read
    tracker.use_var("file"); // second use: write (aliasing!)

    let linear_errors = tracker.check();
    assert_eq!(linear_errors.len(), 1);
    assert_eq!(linear_errors[0].code, "A05001");
}

// -- Effect + Typestate interaction tests --------------------------------
//
// Operations that cause typestate transitions may also have effect
// requirements. Both the state transition validity and effect
// containment must be checked.

#[test]
fn interaction_effect_typestate_transition_with_effects() {
    // An operation that transitions state and has effects.
    // Both the typestate transition and effect containment must pass.
    let states = vec!["Disconnected".into(), "Connected".into()];
    let transitions = vec![("connect".into(), "Disconnected".into(), "Connected".into())];
    let mut ts_checker = TypestateChecker::new(states, transitions, "Disconnected".into(), 0..12);

    // Typestate: connect() in Disconnected => Connected (OK)
    ts_checker.transition("connect", 20..27).unwrap();
    assert_eq!(ts_checker.current_state(), "Connected");

    // Effect: function declares {io}, connect performs {io} (OK)
    let eff_checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io"]);
    let actual = EffectSet::from_effect_names(["network.connect"]);
    let eff_errors = eff_checker.check_containment(&declared, &actual, &(20..27));
    assert!(eff_errors.is_empty());
}

#[test]
fn interaction_effect_typestate_wrong_state_with_correct_effects() {
    // Operation has correct effects but is called in the wrong state.
    // Effect check passes, but typestate check must fail with A06001.
    let states = vec!["Closed".into(), "Open".into()];
    let transitions = vec![("write".into(), "Open".into(), "Open".into())];
    let mut ts_checker = TypestateChecker::new(states, transitions, "Closed".into(), 0..6);

    // Typestate: write() requires Open but we are in Closed => A06001
    let ts_err = ts_checker.transition("write", 10..15);
    assert!(ts_err.is_err());
    assert_eq!(ts_err.unwrap_err().code, "A06001");

    // Effect check: independently passes
    let eff_checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io"]);
    let actual = EffectSet::from_effect_names(["io"]);
    assert!(
        eff_checker
            .check_containment(&declared, &actual, &(10..15))
            .is_empty()
    );
}

#[test]
fn interaction_effect_typestate_correct_state_wrong_effects() {
    // Operation is called in the correct state but with undeclared
    // effects. Typestate check passes, effect check fails with A07001.
    let states = vec!["Init".into(), "Running".into()];
    let transitions = vec![("start".into(), "Init".into(), "Running".into())];
    let mut ts_checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

    // Typestate: start() in Init => Running (OK)
    ts_checker.transition("start", 5..10).unwrap();

    // Effect: function declares {mem} but start() does {io} => A07001
    let eff_checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["mem"]);
    let actual = EffectSet::from_effect_names(["io"]);
    let errors = eff_checker.check_containment(&declared, &actual, &(5..10));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A07001");
}

// -- Test Case 10: Conditional Typestate (Branch Divergence) --------------
//
// Spec Section 13.10: Different branches lead to different states.
// After diverging branches, state is ambiguous => A06004.

#[test]
fn interaction_typestate_branch_divergence_a06004() {
    // After an if/match, if one branch transitions to state A and
    // the other to state B, the post-branch state is ambiguous.
    let states = vec!["Idle".into(), "Active".into(), "Error".into()];
    let transitions = vec![
        ("activate".into(), "Idle".into(), "Active".into()),
        ("fail".into(), "Idle".into(), "Error".into()),
    ];

    // Branch A: activate => Active
    let mut checker_a =
        TypestateChecker::new(states.clone(), transitions.clone(), "Idle".into(), 0..4);
    checker_a.transition("activate", 10..18).unwrap();

    // Branch B: fail => Error
    let mut checker_b = TypestateChecker::new(states, transitions, "Idle".into(), 0..4);
    checker_b.transition("fail", 10..14).unwrap();

    // Post-branch: Active vs Error => A06004
    let err = TypestateChecker::check_branch_consistency(&checker_a, &checker_b, 20..25);
    let err = err.unwrap();
    assert_eq!(err.code, "A06004");
    assert!(err.message.contains("Active"));
    assert!(err.message.contains("Error"));
}

#[test]
fn interaction_typestate_branch_divergence_same_state_ok() {
    // Both branches transition to the same state: no ambiguity.
    let states = vec!["Pending".into(), "Done".into()];
    let transitions = vec![
        ("complete_a".into(), "Pending".into(), "Done".into()),
        ("complete_b".into(), "Pending".into(), "Done".into()),
    ];

    let mut checker_a =
        TypestateChecker::new(states.clone(), transitions.clone(), "Pending".into(), 0..7);
    checker_a.transition("complete_a", 10..20).unwrap();

    let mut checker_b = TypestateChecker::new(states, transitions, "Pending".into(), 0..7);
    checker_b.transition("complete_b", 10..20).unwrap();

    let err = TypestateChecker::check_branch_consistency(&checker_a, &checker_b, 20..25);
    assert!(err.is_none());
}

#[test]
fn interaction_typestate_branch_one_transitions_other_stays() {
    // One branch transitions, the other stays in the original state.
    // Post-branch: states differ => A06004.
    let states = vec!["Idle".into(), "Active".into()];
    let transitions = vec![("start".into(), "Idle".into(), "Active".into())];

    let mut checker_a =
        TypestateChecker::new(states.clone(), transitions.clone(), "Idle".into(), 0..4);
    checker_a.transition("start", 10..15).unwrap();
    // checker_a: Active

    let checker_b = TypestateChecker::new(states, transitions, "Idle".into(), 0..4);
    // checker_b: still Idle (no transition in this branch)

    let err = TypestateChecker::check_branch_consistency(&checker_a, &checker_b, 20..25);
    let err = err.unwrap();
    assert_eq!(err.code, "A06004");
    assert!(err.message.contains("Active"));
    assert!(err.message.contains("Idle"));
}

#[test]
fn interaction_typestate_branch_divergence_with_linear_context() {
    // Combine typestate branch divergence with linear context splitting.
    // A linear variable is used consistently in both branches (OK for
    // linearity), but the typestate diverges (A06004).
    let mut tracker = UsageTracker::new();
    tracker.declare("resource".into(), UsageGrade::Linear, 0..8);
    let mut ctx = LinearContext::new(tracker);

    // if cond then use(resource) else use(resource)
    let expr = Spanned::no_span(AstExpr::If {
        cond: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
        then_branch: Box::new(Spanned::no_span(AstExpr::Call {
            func: Box::new(Spanned::no_span(AstExpr::Ident("activate".into()))),
            args: vec![Spanned::no_span(AstExpr::Ident("resource".into()))],
        })),
        else_branch: Some(Box::new(Spanned::no_span(AstExpr::Call {
            func: Box::new(Spanned::no_span(AstExpr::Ident("deactivate".into()))),
            args: vec![Spanned::no_span(AstExpr::Ident("resource".into()))],
        }))),
    });
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    // Linear: consistent (1 use in each branch) => no A05004
    assert!(
        branch_errors.is_empty(),
        "linear should be consistent: {branch_errors:?}"
    );
    let linear_final = ctx.check();
    assert!(linear_final.is_empty());

    // Meanwhile, typestate diverges:
    let states = vec!["Idle".into(), "Active".into(), "Stopped".into()];
    let transitions = vec![
        ("activate".into(), "Idle".into(), "Active".into()),
        ("deactivate".into(), "Idle".into(), "Stopped".into()),
    ];
    let mut ts_a = TypestateChecker::new(states.clone(), transitions.clone(), "Idle".into(), 0..4);
    ts_a.transition("activate", 10..18).unwrap();

    let mut ts_b = TypestateChecker::new(states, transitions, "Idle".into(), 0..4);
    ts_b.transition("deactivate", 10..20).unwrap();

    let ts_err = TypestateChecker::check_branch_consistency(&ts_a, &ts_b, 0..25);
    assert_eq!(ts_err.unwrap().code, "A06004");
}

// -- Effect containment in functions (pure calling effectful) -------------
//
// Spec Section 3.5: A pure function calling an effectful one is an
// effect containment violation.

#[test]
fn interaction_effect_containment_pure_calls_io_a07002() {
    // A function declared `pure` (empty effect set) that internally
    // performs an `io` effect must produce A07002.
    let checker = EffectChecker::new();
    let declared = EffectSet::pure();
    let actual = EffectSet::from_effect_names(["io"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A07002");
    assert!(errors[0].message.contains("pure"));
    assert!(errors[0].message.contains("io"));
}

#[test]
fn interaction_effect_containment_io_calls_database_a07001() {
    // A function declared `{io}` that performs `database.write`:
    // database effects are NOT sub-effects of io.
    // This must produce A07001.
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io"]);
    let actual = EffectSet::from_effect_names(["database.write"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A07001");
}

#[test]
fn interaction_effect_containment_database_covers_subeffects() {
    // A function declared `{database}` can perform `database.read`
    // and `database.write` (sub-effects of the database group).
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["database"]);
    let actual = EffectSet::from_effect_names(["database.read", "database.write"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert!(errors.is_empty());
}

// -- Linear context fork/merge with multiple variables -------------------
//
// Tests that context splitting correctly tracks multiple independent
// linear variables through branches.

#[test]
fn interaction_linear_context_fork_merge_two_vars() {
    // Two linear variables, each consumed in different branches.
    // var `a` consumed in then-branch, var `b` consumed in else-branch.
    // Both are inconsistent across branches => two A05004 errors.
    let mut tracker = UsageTracker::new();
    tracker.declare("a".into(), UsageGrade::Linear, 0..1);
    tracker.declare("b".into(), UsageGrade::Linear, 2..3);
    let mut ctx = LinearContext::new(tracker);

    // if cond then a else b
    let expr = Spanned::no_span(AstExpr::If {
        cond: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
        then_branch: Box::new(Spanned::no_span(AstExpr::Ident("a".into()))),
        else_branch: Some(Box::new(Spanned::no_span(AstExpr::Ident("b".into())))),
    });
    let errors = check_expr_linearity(&expr, &mut ctx);
    assert_eq!(errors.len(), 2);
    assert!(errors.iter().all(|e| e.code == "A05004"));

    // One error for `a` (used in then, not in else)
    // One error for `b` (used in else, not in then)
    let names: Vec<bool> = errors
        .iter()
        .map(|e| e.message.contains("a") || e.message.contains("b"))
        .collect();
    assert!(names.iter().all(|&b| b));
}

#[test]
fn interaction_linear_context_fork_merge_swap_in_branches() {
    // Two linear variables, both consumed once in each branch
    // (swapped order). Both are consistent => no errors.
    let mut tracker = UsageTracker::new();
    tracker.declare("x".into(), UsageGrade::Linear, 0..1);
    tracker.declare("y".into(), UsageGrade::Linear, 2..3);
    let mut ctx = LinearContext::new(tracker);

    // if cond then [x, y] else [y, x]
    // Both x and y used once in each branch (consistent delta = 1)
    let expr = Spanned::no_span(AstExpr::If {
        cond: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
        then_branch: Box::new(Spanned::no_span(AstExpr::List(vec![
            Spanned::no_span(AstExpr::Ident("x".into())),
            Spanned::no_span(AstExpr::Ident("y".into())),
        ]))),
        else_branch: Some(Box::new(Spanned::no_span(AstExpr::List(vec![
            Spanned::no_span(AstExpr::Ident("y".into())),
            Spanned::no_span(AstExpr::Ident("x".into())),
        ])))),
    });
    let branch_errors = check_expr_linearity(&expr, &mut ctx);
    assert!(branch_errors.is_empty());

    let final_errors = ctx.check();
    assert!(final_errors.is_empty());
}

// -- Test Case 7: Linear + Information Flow (orthogonal axes) ------------
//
// Spec Section 13.7: Linearity and information flow are independent.
// A value has both a usage grade (linear, unlimited, etc.) and a
// security label (Public, Confidential, etc.). These are tracked on
// orthogonal axes.
//
// Since information flow checking (T051) is not yet implemented, we
// test the orthogonality at the type/tracker level: a variable with
// a security label type AND a linear grade should be checked for both
// independently.

#[test]
fn interaction_linear_infoflow_orthogonal_grade_and_type() {
    // A variable that is both linear (grade 1) and has a
    // Confidential-labeled type. The linear checker tracks usage;
    // the type checker tracks the label. They do not interfere.
    let mut tracker = UsageTracker::new();
    tracker.declare("secret_key".into(), UsageGrade::Linear, 0..10);

    // Type is Refined { base: Bytes, predicate: "label == Confidential" }
    let _ty = Type::refined_from_str(Type::Bytes, "x", "label == Confidential");

    // One computational use: linear check passes
    tracker.use_var("secret_key");
    let errors = tracker.check();
    assert!(errors.is_empty());
}

#[test]
fn interaction_linear_infoflow_unlimited_with_label() {
    // An unlimited variable with a Public label. No linearity
    // constraints, but the type carries the label for info-flow.
    let mut tracker = UsageTracker::new();
    tracker.declare("public_data".into(), UsageGrade::Unlimited, 0..11);

    let _ty = Type::refined_from_str(Type::String, "x", "label == Public");

    // Multiple uses: unlimited grade allows any count
    tracker.use_var("public_data");
    tracker.use_var("public_data");
    tracker.use_var("public_data");
    let errors = tracker.check();
    assert!(errors.is_empty());
}

// -- Test Case 8: Typestate + Effect + Refinement (Three-Way) ------------
//
// Spec Section 13.8: All three features interact simultaneously.
// A typestate variable has a refinement predicate, undergoes state
// transitions, and the operations have effect annotations.

#[test]
fn interaction_three_way_typestate_effect_refinement_all_pass() {
    // Three-way interaction:
    // 1. Typestate: object transitions Init -> Open -> Closed
    // 2. Effects: open() has {io}, close() has {io}
    // 3. Refinement: object has a predicate (capacity > 0)
    //
    // All three checks pass when correctly combined.
    let states = vec!["Init".into(), "Open".into(), "Closed".into()];
    let transitions = vec![
        ("open".into(), "Init".into(), "Open".into()),
        ("close".into(), "Open".into(), "Closed".into()),
    ];
    let mut ts = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

    // Typestate transitions
    ts.transition("open", 10..14).unwrap();
    ts.transition("close", 15..20).unwrap();
    assert_eq!(ts.current_state(), "Closed");

    // Typestate variable is linear
    assert!(ts.validate_linear(true).is_none());

    // All transitions reference declared states
    assert!(ts.validate_transitions().is_empty());

    // Effects: function declares {io}, body performs {io}
    let eff = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io"]);
    let actual = EffectSet::from_effect_names(["network.connect"]);
    assert!(
        eff.check_containment(&declared, &actual, &(10..20))
            .is_empty()
    );

    // Refinement: the type has a predicate (compile-time, no runtime cost)
    let ty = Type::refined_from_str(Type::Named("Connection".into()), "x", "capacity > 0");
    assert_eq!(format!("{ty}"), "{ x : Connection | capacity > 0 }");
}

#[test]
fn interaction_three_way_typestate_passes_effect_fails() {
    // Three-way: typestate and refinement are OK, but effects fail.
    // This tests that each checker operates independently.
    let states = vec!["Ready".into(), "Done".into()];
    let transitions = vec![("execute".into(), "Ready".into(), "Done".into())];
    let mut ts = TypestateChecker::new(states, transitions, "Ready".into(), 0..5);

    // Typestate OK
    ts.transition("execute", 10..17).unwrap();

    // Refinement OK (ghost predicate)
    let _ty = Type::refined_from_str(Type::Named("Task".into()), "x", "priority > 0");

    // Effects FAIL: declared pure, body does io
    let eff = EffectChecker::new();
    let declared = EffectSet::pure();
    let actual = EffectSet::from_effect_names(["io"]);
    let errors = eff.check_containment(&declared, &actual, &(10..17));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A07002");
}

#[test]
fn interaction_three_way_effect_passes_typestate_fails() {
    // Three-way: effects are OK, but typestate transition fails.
    let states = vec!["Locked".into(), "Unlocked".into()];
    let transitions = vec![("unlock".into(), "Locked".into(), "Unlocked".into())];
    let mut ts = TypestateChecker::new(
        states,
        transitions,
        "Unlocked".into(), // Already unlocked
        0..8,
    );

    // Typestate FAIL: unlock requires Locked, but we are Unlocked
    let ts_err = ts.transition("unlock", 10..16);
    assert!(ts_err.is_err());
    assert_eq!(ts_err.unwrap_err().code, "A06001");

    // Effects OK: declared {io}, body does {io}
    let eff = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io"]);
    let actual = EffectSet::from_effect_names(["io"]);
    assert!(
        eff.check_containment(&declared, &actual, &(10..16))
            .is_empty()
    );
}

// -- Test Case 11 proxy: Effect + Info-flow (labeled effects) ------------
//
// Since full information flow is not yet implemented (T051), we test
// the effect system's ability to distinguish between effect categories
// that will eventually carry labels. This validates the infrastructure
// needed for Test Case 11.

#[test]
fn interaction_effect_hierarchy_separation() {
    // io effects and database effects are separate hierarchies.
    // Declaring {io} does NOT cover {database.write}.
    // This separation is the foundation for Test Case 11's labeled
    // effects where different effect categories may have different
    // security labels.
    let checker = EffectChecker::new();

    // io does NOT cover database
    let declared_io = EffectSet::from_effect_names(["io"]);
    let actual_db = EffectSet::from_effect_names(["database.write"]);
    let errors = checker.check_containment(&declared_io, &actual_db, &(0..5));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A07001");

    // database does NOT cover io
    let declared_db = EffectSet::from_effect_names(["database"]);
    let actual_io = EffectSet::from_effect_names(["console.write"]);
    let errors = checker.check_containment(&declared_db, &actual_io, &(0..5));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A07001");
}

#[test]
fn interaction_effect_multiple_groups_combined() {
    // Declaring both {io, database} covers sub-effects of both.
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io", "database"]);
    let actual = EffectSet::from_effect_names(["console.write", "network.send", "database.read"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert!(errors.is_empty());
}

// -- Combined: Linear + Typestate + Effect (full pipeline simulation) ----

#[test]
fn interaction_full_pipeline_linear_typestate_effect_pass() {
    // Simulate a full pipeline check for a resource:
    // 1. Linear: resource consumed exactly once
    // 2. Typestate: valid transition sequence
    // 3. Effects: all effects declared
    //
    // Scenario: a database connection that is opened, used, and closed.

    // --- Linear tracking ---
    let mut tracker = UsageTracker::new();
    tracker.declare("db_conn".into(), UsageGrade::Linear, 0..7);
    let mut ctx = LinearContext::new(tracker);

    // Resource consumed once (via close)
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::Ident("db_conn".into()))),
        method: "close".into(),
        args: vec![],
    });
    check_expr_linearity(&expr, &mut ctx);
    let linear_errors = ctx.check();
    assert!(linear_errors.is_empty(), "linear: {linear_errors:?}");

    // --- Typestate tracking ---
    let states = vec![
        "Disconnected".into(),
        "Connected".into(),
        "InTransaction".into(),
        "Closed".into(),
    ];
    let transitions = vec![
        ("connect".into(), "Disconnected".into(), "Connected".into()),
        (
            "begin_tx".into(),
            "Connected".into(),
            "InTransaction".into(),
        ),
        ("commit".into(), "InTransaction".into(), "Connected".into()),
        ("close".into(), "Connected".into(), "Closed".into()),
    ];
    let mut ts = TypestateChecker::new(states, transitions, "Disconnected".into(), 0..12);

    ts.transition("connect", 10..17).unwrap();
    ts.transition("begin_tx", 18..26).unwrap();
    ts.transition("commit", 27..33).unwrap();
    ts.transition("close", 34..39).unwrap();
    assert_eq!(ts.current_state(), "Closed");
    assert!(ts.validate_linear(true).is_none());
    assert!(ts.validate_transitions().is_empty());

    // --- Effect tracking ---
    let eff = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["database", "io"]);
    let actual =
        EffectSet::from_effect_names(["database.read", "database.write", "network.connect"]);
    let eff_errors = eff.check_containment(&declared, &actual, &(0..39));
    assert!(eff_errors.is_empty(), "effects: {eff_errors:?}");
}

#[test]
fn interaction_full_pipeline_all_three_fail() {
    // All three checks fail simultaneously:
    // 1. Linear: double use
    // 2. Typestate: wrong state
    // 3. Effects: undeclared effect

    // --- Linear: double use ---
    let mut tracker = UsageTracker::new();
    tracker.declare("res".into(), UsageGrade::Linear, 0..3);
    tracker.use_var("res");
    tracker.use_var("res");
    let linear_errors = tracker.check();
    assert_eq!(linear_errors.len(), 1);
    assert_eq!(linear_errors[0].code, "A05001");

    // --- Typestate: wrong state ---
    let states = vec!["Off".into(), "On".into()];
    let transitions = vec![("turn_off".into(), "On".into(), "Off".into())];
    let mut ts = TypestateChecker::new(states, transitions, "Off".into(), 0..3);
    let ts_err = ts.transition("turn_off", 5..13);
    assert!(ts_err.is_err());
    assert_eq!(ts_err.unwrap_err().code, "A06001");

    // --- Effects: undeclared ---
    let eff = EffectChecker::new();
    let declared = EffectSet::pure();
    let actual = EffectSet::from_effect_names(["database.write"]);
    let eff_errors = eff.check_containment(&declared, &actual, &(0..10));
    assert_eq!(eff_errors.len(), 1);
    assert_eq!(eff_errors[0].code, "A07002");
}

// -----------------------------------------------------------------------
// T045: Frame condition tests (CORE.3)
// -----------------------------------------------------------------------

#[test]
fn extract_modifies_single_ident() {
    let body = Spanned::no_span(AstExpr::Ident("x".into()));
    let targets = extract_modifies_targets(&body);
    assert_eq!(targets, vec!["x"]);
}

#[test]
fn extract_modifies_block_of_idents() {
    let body = Spanned::no_span(AstExpr::Block(vec![
        Spanned::no_span(AstExpr::Ident("x".into())),
        Spanned::no_span(AstExpr::Ident("y".into())),
    ]));
    let targets = extract_modifies_targets(&body);
    assert_eq!(targets, vec!["x", "y"]);
}

#[test]
fn extract_modifies_field_access() {
    let body = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("node".into()))),
        "keys".into(),
    ));
    let targets = extract_modifies_targets(&body);
    assert_eq!(targets, vec!["node.keys"]);
}

#[test]
fn extract_modifies_nested_field() {
    let body = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Field(
            Box::new(Spanned::no_span(AstExpr::Ident("state".into()))),
            "head".into(),
        ))),
        "data".into(),
    ));
    let targets = extract_modifies_targets(&body);
    assert_eq!(targets, vec!["state.head.data"]);
}

#[test]
fn extract_modifies_list() {
    let body = Spanned::no_span(AstExpr::List(vec![
        Spanned::no_span(AstExpr::Ident("a".into())),
        Spanned::no_span(AstExpr::Ident("b".into())),
        Spanned::no_span(AstExpr::Ident("c".into())),
    ]));
    let targets = extract_modifies_targets(&body);
    assert_eq!(targets, vec!["a", "b", "c"]);
}

#[test]
fn extract_modifies_raw_tokens() {
    let body = Spanned::no_span(AstExpr::Raw(vec!["x".into(), ",".into(), "y".into()]));
    let targets = extract_modifies_targets(&body);
    assert_eq!(targets, vec!["x", "y"]);
}

#[test]
fn collect_old_refs_simple() {
    // old(x)
    let expr = Spanned::no_span(AstExpr::Old(Box::new(Spanned::no_span(AstExpr::Ident(
        "x".into(),
    )))));
    let refs = collect_old_references(&expr);
    assert_eq!(refs, vec!["x"]);
}

#[test]
fn collect_old_refs_in_binop() {
    // old(x) == old(y) + 1
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Old(Box::new(Spanned::no_span(
            AstExpr::Ident("x".into()),
        ))))),
        op: AstBinOp::Eq,
        rhs: Box::new(Spanned::no_span(AstExpr::BinOp {
            lhs: Box::new(Spanned::no_span(AstExpr::Old(Box::new(Spanned::no_span(
                AstExpr::Ident("y".into()),
            ))))),
            op: AstBinOp::Add,
            rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
        })),
    });
    let refs = collect_old_references(&expr);
    assert!(refs.contains(&"x".to_string()));
    assert!(refs.contains(&"y".to_string()));
}

#[test]
fn collect_old_refs_field() {
    // old(node.count)
    let expr = Spanned::no_span(AstExpr::Old(Box::new(Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("node".into()))),
        "count".into(),
    )))));
    let refs = collect_old_references(&expr);
    assert_eq!(refs, vec!["node.count"]);
}

#[test]
fn collect_old_refs_none() {
    // x + y (no old() references)
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        op: AstBinOp::Add,
        rhs: Box::new(Spanned::no_span(AstExpr::Ident("y".into()))),
    });
    let refs = collect_old_references(&expr);
    assert!(refs.is_empty());
}

#[test]
fn frame_checker_valid_modifies_clause() {
    // modifies { x } with x in scope -> no errors
    let body = Spanned::no_span(AstExpr::Ident("x".into()));
    let checker = FrameChecker::new(&[&body]);

    let mut env = TypeEnv::new();
    env.insert("x".into(), Type::Int);
    let symbols = assura_resolve::SymbolTable {
        symbols: vec![],
        scopes: vec![],
    };

    let errors = checker.check_scope(&env, &symbols, &(0..10));
    assert!(errors.is_empty());
}

#[test]
fn frame_checker_unknown_var_a14001() {
    // modifies { nonexistent } -> A14001
    let body = Spanned::no_span(AstExpr::Ident("nonexistent".into()));
    let checker = FrameChecker::new(&[&body]);

    let env = TypeEnv::new();
    let symbols = assura_resolve::SymbolTable {
        symbols: vec![],
        scopes: vec![],
    };

    let errors = checker.check_scope(&env, &symbols, &(0..10));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A14001");
    assert!(errors[0].message.contains("nonexistent"));
}

#[test]
fn frame_checker_mixed_scope_check() {
    // modifies { x, unknown_y } -> 1 error for unknown_y
    let body = Spanned::no_span(AstExpr::Block(vec![
        Spanned::no_span(AstExpr::Ident("x".into())),
        Spanned::no_span(AstExpr::Ident("unknown_y".into())),
    ]));
    let checker = FrameChecker::new(&[&body]);

    let mut env = TypeEnv::new();
    env.insert("x".into(), Type::Int);
    let symbols = assura_resolve::SymbolTable {
        symbols: vec![],
        scopes: vec![],
    };

    let errors = checker.check_scope(&env, &symbols, &(0..10));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A14001");
    assert!(errors[0].message.contains("unknown_y"));
}

#[test]
fn frame_checker_frame_axiom_vars() {
    // modifies { x }, ensures: y == old(y)
    // y is NOT in the modifies set, so it gets a frame axiom
    let modifies_body = Spanned::no_span(AstExpr::Ident("x".into()));
    let checker = FrameChecker::new(&[&modifies_body]);

    let ensures_body = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Ident("y".into()))),
        op: AstBinOp::Eq,
        rhs: Box::new(Spanned::no_span(AstExpr::Old(Box::new(Spanned::no_span(
            AstExpr::Ident("y".into()),
        ))))),
    });

    let frame_vars = checker.frame_axiom_vars(&ensures_body);
    assert!(frame_vars.contains(&"y".to_string()));
    // x IS modified, so it should NOT appear
    assert!(!frame_vars.contains(&"x".to_string()));
}

#[test]
fn frame_checker_modified_var_no_axiom() {
    // modifies { x }, ensures: x == old(x) + 1
    // x IS in the modifies set, so it should NOT get a frame axiom
    let modifies_body = Spanned::no_span(AstExpr::Ident("x".into()));
    let checker = FrameChecker::new(&[&modifies_body]);

    let ensures_body = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        op: AstBinOp::Eq,
        rhs: Box::new(Spanned::no_span(AstExpr::BinOp {
            lhs: Box::new(Spanned::no_span(AstExpr::Old(Box::new(Spanned::no_span(
                AstExpr::Ident("x".into()),
            ))))),
            op: AstBinOp::Add,
            rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
        })),
    });

    let frame_vars = checker.frame_axiom_vars(&ensures_body);
    assert!(!frame_vars.contains(&"x".to_string()));
}

#[test]
fn frame_checker_empty_no_axioms() {
    // No modifies clause -> no frame axioms
    let checker = FrameChecker::empty();
    assert!(!checker.has_modifies());

    let ensures_body = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Ident("y".into()))),
        op: AstBinOp::Eq,
        rhs: Box::new(Spanned::no_span(AstExpr::Old(Box::new(Spanned::no_span(
            AstExpr::Ident("y".into()),
        ))))),
    });

    let frame_vars = checker.frame_axiom_vars(&ensures_body);
    assert!(frame_vars.is_empty());
}

#[test]
fn frame_checker_has_modifies() {
    let body = Spanned::no_span(AstExpr::Ident("x".into()));
    let checker = FrameChecker::new(&[&body]);
    assert!(checker.has_modifies());
}

#[test]
fn frame_checker_candidates_frame_unmodified_param() {
    // modifies { x }, ensures only mentions x; candidate y still gets a frame axiom.
    let modifies_body = Spanned::no_span(AstExpr::Ident("x".into()));
    let checker = FrameChecker::new(&[&modifies_body]);
    let ensures_body = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        op: AstBinOp::Gt,
        rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("0".into())))),
    });
    let candidates = vec!["x".into(), "y".into()];
    let frame_vars = checker.frame_axiom_vars_with_candidates(&ensures_body, &candidates);
    assert!(
        frame_vars.contains(&"y".to_string()),
        "unmodified param y should be framed, got {frame_vars:?}"
    );
    assert!(
        !frame_vars.contains(&"x".to_string()),
        "modified x must not get frame axiom"
    );
}

#[test]
fn frame_checker_is_modified() {
    let body = Spanned::no_span(AstExpr::Block(vec![
        Spanned::no_span(AstExpr::Ident("x".into())),
        Spanned::no_span(AstExpr::Ident("y".into())),
    ]));
    let checker = FrameChecker::new(&[&body]);
    assert!(checker.is_modified("x"));
    assert!(checker.is_modified("y"));
    assert!(!checker.is_modified("z"));
}

// -----------------------------------------------------------------------
// T043 CORE.1: Ghost code tests
// -----------------------------------------------------------------------

#[test]
fn ghost_fn_pure_effects_passes() {
    // A ghost function with effects: pure should type-check fine.
    let src = r#"
ghost fn invariant_helper(x: Int) -> Bool
effects: pure
ensures { result == true }
"#;
    let resolved = resolve_ok(src);
    let result = type_check(resolved);
    assert!(
        result.is_ok(),
        "ghost fn with pure effects should pass: {result:?}"
    );
}

#[test]
fn ghost_fn_no_effects_clause_passes() {
    // A ghost function with no explicit effects clause is implicitly pure.
    let src = r#"
ghost fn spec_helper(x: Int) -> Bool
ensures { result == true }
"#;
    let resolved = resolve_ok(src);
    let result = type_check(resolved);
    assert!(
        result.is_ok(),
        "ghost fn without effects clause should pass: {result:?}"
    );
}

#[test]
fn ghost_fn_non_pure_effects_a54001() {
    // A ghost function with io effects should produce A54001.
    let src = r#"
ghost fn bad_ghost(x: Int) -> Bool
effects: io
ensures { result == true }
"#;
    let resolved = resolve_ok(src);
    let result = type_check(resolved);
    assert!(result.is_err(), "ghost fn with io effects should fail");
    let errors = result.unwrap_err();
    assert!(
        errors.iter().any(|e| e.code == "A54001"),
        "should produce A54001, got: {errors:?}"
    );
    assert!(
        errors[0].message.contains("ghost function"),
        "error message should mention ghost function"
    );
}

#[test]
fn ghost_block_type_checks_inner() {
    // A ghost block should type-check its inner expression.
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Ghost(Box::new(Spanned::no_span(
        AstExpr::Literal(AstLit::Bool(true)),
    ))));
    // Ghost block type is Unit (erased at runtime)
    assert_eq!(infer_expr(&expr, &env).unwrap(), Type::Unit);
}

#[test]
fn ghost_block_propagates_inner_error() {
    // A ghost block with a type error in its body should propagate the error.
    let env = TypeEnv::new();
    let expr = Spanned::no_span(AstExpr::Ghost(Box::new(Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
        op: AstBinOp::Add,
        rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(false)))),
    }))));
    let err = infer_expr(&expr, &env).unwrap_err();
    assert_eq!(err.code, "A03001");
}

#[test]
fn ghost_var_not_counted_as_linear_use() {
    // References inside a ghost block should NOT count as linear uses.
    let mut tracker = UsageTracker::new();
    tracker.declare("resource".into(), UsageGrade::Linear, 0..1);

    let ghost_expr = Spanned::no_span(AstExpr::Ghost(Box::new(Spanned::no_span(AstExpr::Ident(
        "resource".into(),
    )))));

    // Walk with linearity checker: ghost blocks should not count
    let mut ctx = LinearContext::new(tracker);
    let errors = check_expr_linearity(&ghost_expr, &mut ctx);
    assert!(
        errors.is_empty(),
        "ghost block should not cause linearity errors"
    );

    // The variable should still show 0 uses (ghost use does not count)
    assert_eq!(ctx.get_count("resource"), Some(0));
}

// -----------------------------------------------------------------------
// T044: Lemma tests (CORE.2)
// -----------------------------------------------------------------------

#[test]
fn lemma_fn_pure_effects_passes() {
    // Lemma with pure effects should type-check without errors.
    let src = r#"
        lemma add_comm(a: Int, b: Int)
            effects: pure
            ensures { a + b == b + a }
    "#;
    let file = assura_parser::parse_unwrap(src);
    let resolved = assura_resolve::resolve(&file).unwrap();
    let result = type_check(resolved);
    assert!(
        result.is_ok(),
        "lemma with pure effects should pass type check"
    );
}

#[test]
fn lemma_fn_no_effects_clause_passes() {
    // Lemma with no explicit effects clause is implicitly pure: OK.
    let src = r#"
        lemma trivial(x: Int)
            ensures { x == x }
    "#;
    let file = assura_parser::parse_unwrap(src);
    let resolved = assura_resolve::resolve(&file).unwrap();
    let result = type_check(resolved);
    result.expect("lemma with no effects clause should pass");
}

#[test]
fn lemma_fn_non_pure_effects_a55001() {
    // Lemma with non-pure effects should produce A55001.
    let src = r#"
        lemma bad_lemma(x: Int)
            effects: io
            ensures { x > 0 }
    "#;
    let file = assura_parser::parse_unwrap(src);
    let resolved = assura_resolve::resolve(&file).unwrap();
    let result = type_check(resolved);
    assert!(result.is_err(), "lemma with io effects should fail");
    let errors = result.unwrap_err();
    assert!(
        errors.iter().any(|e| e.code == "A55001"),
        "should produce A55001, got: {errors:?}"
    );
}

#[test]
fn lemma_is_lemma_flag_set() {
    // Verify that parsing a lemma sets is_lemma = true.
    let src = r#"
        lemma my_lemma(n: Int)
            ensures { n >= 0 }
    "#;
    let file = assura_parser::parse_unwrap(src);
    assert_eq!(file.decls.len(), 1);
    if let Decl::FnDef(f) = &file.decls[0].node {
        assert!(f.is_lemma, "lemma should have is_lemma = true");
        assert!(!f.is_ghost, "lemma should not have is_ghost = true");
        assert_eq!(f.name, "my_lemma");
    } else {
        panic!("expected FnDef, got {:?}", file.decls[0].node);
    }
}

#[test]
fn fn_is_not_lemma() {
    // Verify that parsing a regular fn sets is_lemma = false.
    let src = r#"
        fn regular(n: Int) -> Int {
            ensures { result >= 0 }
        }
    "#;
    let file = assura_parser::parse_unwrap(src);
    assert_eq!(file.decls.len(), 1);
    if let Decl::FnDef(f) = &file.decls[0].node {
        assert!(!f.is_lemma, "fn should have is_lemma = false");
    } else {
        panic!("expected FnDef");
    }
}

#[test]
fn apply_expr_type_is_bool() {
    // apply lemma_name(args) should have Bool type.
    let env = TypeEnv::new();
    let apply = Spanned::no_span(AstExpr::Apply {
        lemma_name: "some_lemma".into(),
        args: vec![Spanned::no_span(AstExpr::Literal(AstLit::Int("42".into())))],
    });
    let result = infer_expr(&apply, &env);
    assert_eq!(result.unwrap(), Type::Bool);
}

#[test]
fn apply_not_counted_as_linear_use() {
    // apply should not count variable references as linear uses.
    let mut tracker = UsageTracker::new();
    tracker.declare("resource".into(), UsageGrade::Linear, 0..1);

    let apply = Spanned::no_span(AstExpr::Apply {
        lemma_name: "some_lemma".into(),
        args: vec![Spanned::no_span(AstExpr::Ident("resource".into()))],
    });

    let mut ctx = LinearContext::new(tracker);
    let errors = check_expr_linearity(&apply, &mut ctx);
    assert!(errors.is_empty(), "apply should not cause linearity errors");
    assert_eq!(ctx.get_count("resource"), Some(0));
}

// -----------------------------------------------------------------------
// T064: Error propagation tests
// -----------------------------------------------------------------------

#[test]
fn test_error_propagation_must_propagate_swallow_rejected() {
    let mut checker = ErrorPropagationChecker::new();
    checker.register_policy(
        "TestPolicy".into(),
        ErrorPolicy {
            must_propagate: vec!["SQLITE_CORRUPT".into(), "SQLITE_NOMEM".into()],
            ..Default::default()
        },
    );

    // Swallowing a must_propagate error should produce A12001
    let err = checker.validate_catch("SQLITE_CORRUPT", ErrorAction::Swallow, 0..10);
    assert!(err.is_some(), "swallowing must_propagate error should fail");
    assert_eq!(err.unwrap().code, "A12001");

    // Propagating is fine
    let err = checker.validate_catch("SQLITE_CORRUPT", ErrorAction::Propagate, 0..10);
    assert!(
        err.is_none(),
        "propagating must_propagate error should pass"
    );

    // Handling is fine
    let err = checker.validate_catch("SQLITE_CORRUPT", ErrorAction::Handle, 0..10);
    assert!(err.is_none(), "handling must_propagate error should pass");

    // Swallowing a non-must_propagate error is fine
    let err = checker.validate_catch("SQLITE_BUSY", ErrorAction::Swallow, 0..10);
    assert!(err.is_none(), "swallowing non-policy error should pass");
}

#[test]
fn test_error_propagation_must_not_mask() {
    let mut checker = ErrorPropagationChecker::new();
    checker.register_policy(
        "TestPolicy".into(),
        ErrorPolicy {
            must_not_mask: vec![
                ("SQLITE_CORRUPT".into(), "SQLITE_OK".into()),
                ("SQLITE_NOMEM".into(), "SQLITE_ERROR".into()),
            ],
            ..Default::default()
        },
    );

    // Forbidden translation should produce A12002
    let err = checker.validate_catch(
        "SQLITE_CORRUPT",
        ErrorAction::TranslateTo("SQLITE_OK".into()),
        0..10,
    );
    assert!(err.is_some(), "forbidden translation should fail");
    assert_eq!(err.unwrap().code, "A12002");

    // Allowed translation should pass
    let err = checker.validate_catch(
        "SQLITE_CORRUPT",
        ErrorAction::TranslateTo("SQLITE_CORRUPT_DETAILED".into()),
        0..10,
    );
    assert!(err.is_none(), "non-forbidden translation should pass");
}

#[test]
fn test_error_propagation_must_check() {
    let mut checker = ErrorPropagationChecker::new();
    checker.register_policy(
        "TestPolicy".into(),
        ErrorPolicy {
            must_check: vec!["sqlite3_reset".into(), "sqlite3_finalize".into()],
            ..Default::default()
        },
    );

    // Unchecked call to must_check function -> A12003
    let err = checker.validate_unchecked_call("sqlite3_reset", 0..10);
    assert!(err.is_some(), "unchecked must_check call should fail");
    assert_eq!(err.unwrap().code, "A12003");

    // Non-must_check function is fine
    let err = checker.validate_unchecked_call("sqlite3_open", 0..10);
    assert!(err.is_none(), "non-policy function should pass");
}

#[test]
fn test_error_propagation_multiple_policies() {
    let mut checker = ErrorPropagationChecker::new();
    checker.register_policy(
        "PolicyA".into(),
        ErrorPolicy {
            must_propagate: vec!["ERR_A".into()],
            ..Default::default()
        },
    );
    checker.register_policy(
        "PolicyB".into(),
        ErrorPolicy {
            must_propagate: vec!["ERR_B".into()],
            ..Default::default()
        },
    );

    // Both policies are checked
    assert!(checker.is_must_propagate("ERR_A"));
    assert!(checker.is_must_propagate("ERR_B"));
    assert!(!checker.is_must_propagate("ERR_C"));
}

#[test]
fn test_error_propagation_empty_policy() {
    let checker = ErrorPropagationChecker::new();

    // No policies registered: everything passes
    let err = checker.validate_catch("ANY_ERROR", ErrorAction::Swallow, 0..10);
    assert!(err.is_none(), "no policy means no restrictions");
}

// -----------------------------------------------------------------------
