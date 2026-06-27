use super::*;

// =======================================================================
// T100: UnsafeEscapeChecker tests
// =======================================================================

#[test]
fn unsafe_no_proof() {
    let mut ue = UnsafeEscapeChecker::new();
    ue.declare_unsafe("ptr_deref".into(), vec!["aligned".into()], 0..1);
    let errs = ue.check_unproven();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A47001");
}

#[test]
fn unsafe_with_proof_ok() {
    let mut ue = UnsafeEscapeChecker::new();
    ue.declare_unsafe("ptr_deref".into(), vec!["aligned".into()], 0..1);
    ue.attach_proof("ptr_deref");
    assert!(ue.check_unproven().is_empty());
}

#[test]
fn unsafe_undischarged_obligation() {
    let mut ue = UnsafeEscapeChecker::new();
    ue.declare_unsafe(
        "cast".into(),
        vec!["valid_repr".into(), "aligned".into()],
        0..1,
    );
    ue.discharge_obligation("cast", "valid_repr".into());
    let errs = ue.check_obligations();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A47002");
}

#[test]
fn unsafe_empty_obligations() {
    let mut ue = UnsafeEscapeChecker::new();
    ue.declare_unsafe("noop".into(), vec![], 0..1);
    let errs = ue.check_empty_obligations();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A47003");
}

#[test]
fn unsafe_default() {
    let ue = UnsafeEscapeChecker::default();
    assert!(ue.check_unproven().is_empty());
}

// =======================================================================
// T101: ComplexityBoundChecker tests
// =======================================================================

#[test]
fn complexity_bound_violated() {
    let mut cb = ComplexityBoundChecker::new();
    cb.declare_bound("sort".into(), ComplexityClass::Linear, 0..1);
    cb.record_measured("sort", ComplexityClass::Quadratic);
    let errs = cb.check_bounds();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A48001");
}

#[test]
fn complexity_bound_ok() {
    let mut cb = ComplexityBoundChecker::new();
    cb.declare_bound("lookup".into(), ComplexityClass::Logarithmic, 0..1);
    cb.record_measured("lookup", ComplexityClass::Constant);
    assert!(cb.check_bounds().is_empty());
}

#[test]
fn complexity_unverified() {
    let mut cb = ComplexityBoundChecker::new();
    cb.declare_bound("search".into(), ComplexityClass::Linear, 0..1);
    let errs = cb.check_unverified();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A48002");
}

#[test]
fn complexity_exponential_warning() {
    let mut cb = ComplexityBoundChecker::new();
    cb.declare_bound("brute".into(), ComplexityClass::Exponential, 0..1);
    let errs = cb.check_expensive();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A48003");
}

#[test]
fn complexity_default() {
    let cb = ComplexityBoundChecker::default();
    assert!(cb.check_bounds().is_empty());
}

// =======================================================================
// T102: BehavioralEquivalenceChecker tests
// =======================================================================

#[test]
fn equiv_unverified() {
    let mut be = BehavioralEquivalenceChecker::new();
    be.declare(
        "eq1".into(),
        "sort_a".into(),
        "sort_b".into(),
        "Sortable".into(),
        0..1,
    );
    let errs = be.check_unverified();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A49001");
}

#[test]
fn equiv_verified_ok() {
    let mut be = BehavioralEquivalenceChecker::new();
    be.declare(
        "eq1".into(),
        "sort_a".into(),
        "sort_b".into(),
        "Sortable".into(),
        0..1,
    );
    be.mark_verified("eq1");
    assert!(be.check_unverified().is_empty());
}

#[test]
fn equiv_self_equivalence() {
    let mut be = BehavioralEquivalenceChecker::new();
    be.declare(
        "eq1".into(),
        "sort_a".into(),
        "sort_a".into(),
        "Sortable".into(),
        0..1,
    );
    let errs = be.check_self_equivalence();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A49002");
}

#[test]
fn equiv_no_contract() {
    let mut be = BehavioralEquivalenceChecker::new();
    be.declare("eq1".into(), "a".into(), "b".into(), "".into(), 0..1);
    let errs = be.check_contract_ref();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A49003");
}

#[test]
fn equiv_default() {
    let be = BehavioralEquivalenceChecker::default();
    assert!(be.check_unverified().is_empty());
}

// =======================================================================
// T103: MultiPassRefinementChecker tests
// =======================================================================

#[test]
fn refinement_incomplete() {
    let mut mp = MultiPassRefinementChecker::new();
    mp.add_pass("r1".into(), "spec".into(), "design".into(), 5, 0..1);
    mp.discharge("r1", 3);
    let errs = mp.check_complete();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A50001");
}

#[test]
fn refinement_complete_ok() {
    let mut mp = MultiPassRefinementChecker::new();
    mp.add_pass("r1".into(), "spec".into(), "design".into(), 5, 0..1);
    mp.discharge("r1", 5);
    assert!(mp.check_complete().is_empty());
}

#[test]
fn refinement_chain_gap() {
    let mut mp = MultiPassRefinementChecker::new();
    mp.add_pass("r1".into(), "spec".into(), "design".into(), 1, 0..1);
    mp.add_pass("r2".into(), "impl".into(), "code".into(), 1, 0..1);
    let errs = mp.check_chain();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A50002");
}

#[test]
fn refinement_zero_obligations() {
    let mut mp = MultiPassRefinementChecker::new();
    mp.add_pass("r1".into(), "spec".into(), "design".into(), 0, 0..1);
    let errs = mp.check_non_trivial();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A50003");
}

#[test]
fn refinement_default() {
    let mp = MultiPassRefinementChecker::default();
    assert!(mp.check_non_trivial().is_empty());
}

// =======================================================================
// T104: IncrementalContractChecker tests
// =======================================================================

#[test]
fn incremental_strengthens_precondition() {
    let mut ic = IncrementalContractChecker::new();
    ic.add_version("SafeDiv".into(), 1, 1, 1, 0..1);
    ic.add_version("SafeDiv".into(), 2, 3, 1, 0..1); // more requires = stronger
    let errs = ic.check_precondition_weakening();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A51001");
}

#[test]
fn incremental_weakens_postcondition() {
    let mut ic = IncrementalContractChecker::new();
    ic.add_version("SafeDiv".into(), 1, 1, 3, 0..1);
    ic.add_version("SafeDiv".into(), 2, 1, 1, 0..1); // fewer ensures = weaker
    let errs = ic.check_postcondition_strengthening();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A51002");
}

#[test]
fn incremental_version_gap() {
    let mut ic = IncrementalContractChecker::new();
    ic.add_version("SafeDiv".into(), 1, 1, 1, 0..1);
    ic.add_version("SafeDiv".into(), 5, 1, 1, 0..1);
    let errs = ic.check_version_continuity();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A51003");
}

#[test]
fn incremental_ok() {
    let mut ic = IncrementalContractChecker::new();
    ic.add_version("SafeDiv".into(), 1, 3, 1, 0..1);
    ic.add_version("SafeDiv".into(), 2, 2, 2, 0..1); // weaker pre, stronger post
    assert!(ic.check_precondition_weakening().is_empty());
    assert!(ic.check_postcondition_strengthening().is_empty());
}

#[test]
fn incremental_default() {
    let ic = IncrementalContractChecker::default();
    assert!(ic.check_precondition_weakening().is_empty());
}

// =======================================================================
// T105: ScopedInvariantChecker tests
// =======================================================================

#[test]
fn invariant_double_suspend() {
    let mut si = ScopedInvariantChecker::new();
    si.declare_invariant("inv1".into());
    assert!(si.suspend("inv1").is_none());
    let err = si.suspend("inv1");
    assert_eq!(err.unwrap().code, "A52001");
}

#[test]
fn invariant_suspend_undeclared() {
    let mut si = ScopedInvariantChecker::new();
    let err = si.suspend("unknown");
    assert_eq!(err.unwrap().code, "A52002");
}

#[test]
fn invariant_restore_not_suspended() {
    let mut si = ScopedInvariantChecker::new();
    si.declare_invariant("inv1".into());
    let err = si.restore("inv1");
    assert_eq!(err.unwrap().code, "A52003");
}

#[test]
fn invariant_suspend_restore_ok() {
    let mut si = ScopedInvariantChecker::new();
    si.declare_invariant("inv1".into());
    si.suspend("inv1");
    assert!(si.is_suspended("inv1"));
    si.restore("inv1");
    assert!(!si.is_suspended("inv1"));
    assert!(si.check_all_restored().is_empty());
}

#[test]
fn invariant_still_suspended_at_exit() {
    let mut si = ScopedInvariantChecker::new();
    si.declare_invariant("inv1".into());
    si.suspend("inv1");
    let errs = si.check_all_restored();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A52001");
}

#[test]
fn invariant_default() {
    let si = ScopedInvariantChecker::default();
    assert!(si.check_all_restored().is_empty());
}

// =======================================================================
// T107: StdlibTypes tests
// =======================================================================

#[test]
fn stdlib_has_core_types() {
    let stdlib = StdlibTypes::new();
    let types = stdlib.all_types();
    let names: Vec<&str> = types.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"Pos"));
    assert!(names.contains(&"NonNeg"));
    assert!(names.contains(&"Email"));
    assert!(names.contains(&"Uuid"));
    assert!(!names.contains(&"Unknown"));
}

#[test]
fn stdlib_default() {
    let stdlib = StdlibTypes::default();
    assert!(stdlib.all_types().len() >= 6);
}

// =======================================================================
// T108: CollectionContracts tests
// =======================================================================

#[test]
fn collection_has_standard_ops() {
    let cc = CollectionContracts::new();
    cc.lookup("sort").unwrap();
    cc.lookup("filter").unwrap();
    cc.lookup("map").unwrap();
    cc.lookup("reverse").unwrap();
}

#[test]
fn collection_sort_preserves_length() {
    let cc = CollectionContracts::new();
    let sort = cc.lookup("sort").unwrap();
    assert!(sort.preserves_length);
}

#[test]
fn collection_filter_does_not_preserve_length() {
    let cc = CollectionContracts::new();
    let filter = cc.lookup("filter").unwrap();
    assert!(!filter.preserves_length);
}

#[test]
fn collection_default() {
    let cc = CollectionContracts::default();
    cc.lookup("sort").unwrap();
}

// =======================================================================
// T109: CrudAuthContracts tests
// =======================================================================

#[test]
fn crud_auth_missing_policy() {
    let mut ca = CrudAuthContracts::new();
    ca.add_crud("create_user".into(), CrudType::Create, true);
    let errs = ca.check_auth_coverage();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A53001");
}

#[test]
fn crud_auth_with_policy_ok() {
    let mut ca = CrudAuthContracts::new();
    ca.add_crud("create_user".into(), CrudType::Create, true);
    ca.add_auth_policy("create_user".into(), "admin".into(), false);
    assert!(ca.check_auth_coverage().is_empty());
}

#[test]
fn crud_delete_without_auth() {
    let mut ca = CrudAuthContracts::new();
    ca.add_crud("delete_item".into(), CrudType::Delete, false);
    let errs = ca.check_delete_protection();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A53002");
}

#[test]
fn crud_default() {
    let ca = CrudAuthContracts::default();
    assert!(ca.check_delete_protection().is_empty());
}

// =======================================================================
// T110: ContractCompositionChecker tests
// =======================================================================

#[test]
fn composition_unknown_extends() {
    let mut cc = ContractCompositionChecker::new();
    cc.declare("Child".into(), vec!["Unknown".into()], 1);
    let errs = cc.check_extends();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A54001");
}

#[test]
fn composition_valid_extends() {
    let mut cc = ContractCompositionChecker::new();
    cc.declare("Base".into(), vec![], 2);
    cc.declare("Child".into(), vec!["Base".into()], 1);
    assert!(cc.check_extends().is_empty());
}

#[test]
fn composition_circular() {
    let mut cc = ContractCompositionChecker::new();
    cc.declare("A".into(), vec!["B".into()], 1);
    cc.declare("B".into(), vec!["A".into()], 1);
    let errs = cc.check_circular();
    assert!(!errs.is_empty());
    assert!(errs.iter().any(|e| e.code == "A54002"));
}

#[test]
fn composition_diamond() {
    let mut cc = ContractCompositionChecker::new();
    cc.declare("Base".into(), vec![], 1);
    cc.declare("Left".into(), vec!["Base".into()], 1);
    cc.declare("Right".into(), vec!["Base".into()], 1);
    cc.declare("Diamond".into(), vec!["Left".into(), "Right".into()], 1);
    let errs = cc.check_diamond();
    assert!(!errs.is_empty());
    assert!(errs.iter().any(|e| e.code == "A54003"));
}

#[test]
fn composition_default() {
    let cc = ContractCompositionChecker::default();
    assert!(cc.check_extends().is_empty());
}

// =======================================================================
// T111: ContractLibraryChecker tests
// =======================================================================

#[test]
fn library_empty_exports() {
    let mut lc = ContractLibraryChecker::new();
    lc.declare_library("mylib".into(), "1.0.0".into());
    let errs = lc.check_empty_exports();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A55001");
}

#[test]
fn library_with_exports_ok() {
    let mut lc = ContractLibraryChecker::new();
    lc.declare_library("mylib".into(), "1.0.0".into());
    lc.add_export("mylib", "SafeDiv".into());
    assert!(lc.check_empty_exports().is_empty());
}

#[test]
fn library_self_dependency() {
    let mut lc = ContractLibraryChecker::new();
    lc.declare_library("mylib".into(), "1.0.0".into());
    lc.add_dependency(
        "mylib",
        LibraryDep {
            name: "mylib".into(),
            version_req: ">=1.0".into(),
        },
    );
    let errs = lc.check_circular_deps();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A55002");
}

#[test]
fn library_duplicate() {
    let mut lc = ContractLibraryChecker::new();
    lc.declare_library("mylib".into(), "1.0.0".into());
    lc.declare_library("mylib".into(), "2.0.0".into());
    let errs = lc.check_duplicates();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A55003");
}

#[test]
fn library_default() {
    let lc = ContractLibraryChecker::default();
    assert!(lc.check_empty_exports().is_empty());
}

// =======================================================================
// Additional coverage tests for issue #149
// Target: 10+ tests per checker struct
// =======================================================================

// -----------------------------------------------------------------------
// ProtocolGrammarChecker additional tests
// -----------------------------------------------------------------------

#[test]
fn protocol_current_state() {
    let mut checker = ProtocolGrammarChecker::new("idle".into());
    checker.add_state("active".into());
    checker.add_transition("idle".into(), "active".into(), "GO".into());
    // From idle, GO is valid
    assert!(checker.check_send("GO", &(0..1)).is_none());
    // STOP is not valid from idle
    checker.check_send("STOP", &(0..1)).unwrap();
}

#[test]
fn protocol_transition_updates_state() {
    let mut checker = ProtocolGrammarChecker::new("idle".into());
    checker.add_state("connected".into());
    checker.add_transition("idle".into(), "connected".into(), "CONNECT".into());
    assert!(checker.transition("CONNECT", &(0..1)).is_none());
    // After transition, CONNECT should no longer be valid (we're in "connected")
    checker.check_send("CONNECT", &(0..1)).unwrap();
}

#[test]
fn protocol_send_valid_no_error() {
    let mut checker = ProtocolGrammarChecker::new("idle".into());
    checker.add_state("ready".into());
    checker.add_transition("idle".into(), "ready".into(), "INIT".into());
    assert!(checker.check_send("INIT", &(0..1)).is_none());
}

#[test]
fn protocol_send_after_transition() {
    let mut checker = ProtocolGrammarChecker::new("idle".into());
    checker.add_state("connected".into());
    checker.add_state("ready".into());
    checker.add_transition("idle".into(), "connected".into(), "CONNECT".into());
    checker.add_transition("connected".into(), "ready".into(), "SETUP".into());
    checker.transition("CONNECT", &(0..1));
    // In connected state, SETUP is valid
    assert!(checker.check_send("SETUP", &(0..1)).is_none());
    // But CONNECT is no longer valid from connected state
    let err = checker.check_send("CONNECT", &(0..1));
    assert_eq!(err.unwrap().code, "A30002");
}

#[test]
fn protocol_required_fields_all_present() {
    let mut checker = ProtocolGrammarChecker::new("idle".into());
    checker.add_required_fields(
        "LOGIN".into(),
        vec!["username".into(), "password".into(), "token".into()],
    );
    let errors =
        checker.check_required_fields("LOGIN", &["username", "password", "token"], &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn protocol_required_fields_multiple_missing() {
    let mut checker = ProtocolGrammarChecker::new("idle".into());
    checker.add_required_fields(
        "DATA".into(),
        vec!["payload".into(), "checksum".into(), "seq".into()],
    );
    let errors = checker.check_required_fields("DATA", &[], &(0..1));
    assert_eq!(errors.len(), 3);
    assert!(errors.iter().all(|e| e.code == "A30003"));
}

#[test]
fn protocol_no_required_fields_defined() {
    let checker = ProtocolGrammarChecker::new("idle".into());
    // No required fields registered for MSG -> empty errors
    let errors = checker.check_required_fields("MSG", &["data"], &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn protocol_cycle_back_to_initial() {
    let mut checker = ProtocolGrammarChecker::new("idle".into());
    checker.add_state("active".into());
    checker.add_transition("idle".into(), "active".into(), "START".into());
    checker.add_transition("active".into(), "idle".into(), "STOP".into());
    assert!(checker.transition("START", &(0..1)).is_none());
    // After transition to active, STOP should be valid
    assert!(checker.check_send("STOP", &(0..1)).is_none());
    assert!(checker.transition("STOP", &(0..1)).is_none());
    // Back to idle, START should be valid again
    assert!(checker.check_send("START", &(0..1)).is_none());
    // Can restart
    assert!(checker.transition("START", &(0..1)).is_none());
    // Now in active again, STOP is valid
    assert!(checker.check_send("STOP", &(0..1)).is_none());
}

// -----------------------------------------------------------------------
// TaintChecker additional tests
// -----------------------------------------------------------------------

#[test]
fn taint_checker_register_validator() {
    let mut checker = TaintChecker::new();
    checker.register_validator("custom_validate".into());
    checker.declare("raw".into(), TaintLabel::Untrusted);
    let expr = Spanned::no_span(AstExpr::Call {
        func: Box::new(Spanned::no_span(AstExpr::Ident("custom_validate".into()))),
        args: vec![Spanned::no_span(AstExpr::Ident("raw".into()))],
    });
    assert_eq!(checker.infer_taint(&expr), TaintLabel::Validated);
}

#[test]
fn taint_checker_sanitize_is_builtin_validator() {
    let checker = TaintChecker::new();
    let expr = Spanned::no_span(AstExpr::Call {
        func: Box::new(Spanned::no_span(AstExpr::Ident("sanitize".into()))),
        args: vec![Spanned::no_span(AstExpr::Ident("input".into()))],
    });
    assert_eq!(checker.infer_taint(&expr), TaintLabel::Validated);
}

#[test]
fn taint_infer_field_propagates() {
    let mut checker = TaintChecker::new();
    checker.declare("req".into(), TaintLabel::Untrusted);
    let expr = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("req".into()))),
        "body".into(),
    ));
    assert_eq!(checker.infer_taint(&expr), TaintLabel::Untrusted);
}

#[test]
fn taint_infer_method_call_validation() {
    let mut checker = TaintChecker::new();
    checker.register_validator("clean".into());
    checker.declare("x".into(), TaintLabel::Untrusted);
    let expr = Spanned::no_span(AstExpr::MethodCall {
        receiver: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        method: "clean".into(),
        args: vec![],
    });
    assert_eq!(checker.infer_taint(&expr), TaintLabel::Validated);
}

#[test]
fn taint_infer_if_expression_propagates() {
    let mut checker = TaintChecker::new();
    checker.declare("cond".into(), TaintLabel::Trusted);
    checker.declare("a".into(), TaintLabel::Untrusted);
    let expr = Spanned::no_span(AstExpr::If {
        cond: Box::new(Spanned::no_span(AstExpr::Ident("cond".into()))),
        then_branch: Box::new(Spanned::no_span(AstExpr::Ident("a".into()))),
        else_branch: Some(Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int(
            "0".into(),
        ))))),
    });
    // min(Trusted, Untrusted, Trusted) = Untrusted
    assert_eq!(checker.infer_taint(&expr), TaintLabel::Untrusted);
}

#[test]
fn taint_infer_list_propagates() {
    let mut checker = TaintChecker::new();
    checker.declare("bad".into(), TaintLabel::Untrusted);
    let expr = Spanned::no_span(AstExpr::List(vec![
        Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into()))),
        Spanned::no_span(AstExpr::Ident("bad".into())),
    ]));
    assert_eq!(checker.infer_taint(&expr), TaintLabel::Untrusted);
}

#[test]
fn taint_checker_has_taint_info() {
    let mut checker = TaintChecker::new();
    assert!(!checker.has_taint_info());
    checker.declare("x".into(), TaintLabel::Untrusted);
    assert!(checker.has_taint_info());
}

#[test]
fn taint_checker_get_label() {
    let mut checker = TaintChecker::new();
    checker.declare("x".into(), TaintLabel::Validated);
    assert_eq!(checker.get_label("x"), Some(TaintLabel::Validated));
    assert_eq!(checker.get_label("y"), None);
}

#[test]
fn taint_checker_alloc_validated_ok() {
    let mut checker = TaintChecker::new();
    checker.declare("sz".into(), TaintLabel::Validated);
    let expr = Spanned::no_span(AstExpr::Call {
        func: Box::new(Spanned::no_span(AstExpr::Ident("malloc".into()))),
        args: vec![Spanned::no_span(AstExpr::Ident("sz".into()))],
    });
    let errors = checker.check_expr(&expr, &(0..1));
    assert!(errors.is_empty(), "validated alloc size should pass");
}

// -----------------------------------------------------------------------
// TotalityChecker additional tests
// -----------------------------------------------------------------------

#[test]
fn totality_new_default() {
    let checker = TotalityChecker::default();
    // Non-recursive function is trivially total
    let f = AstFnDef {
        name: "foo".into(),
        params: vec![],
        return_ty: None,
        clauses: vec![],
        is_ghost: false,
        is_lemma: false,
    };
    let (errors, pending) = checker.check_function_totality(&f, &(0..1));
    assert!(errors.is_empty());
    assert!(pending.is_empty());
}

#[test]
fn totality_partial_fn_skipped() {
    let mut checker = TotalityChecker::new();
    checker.mark_partial("loop_forever".into());
    // Even with recursive calls, partial functions are skipped
    let f = AstFnDef {
        name: "loop_forever".into(),
        params: vec![AstParam {
            name: "n".into(),
            ty: assura_parser::ast::try_parse_type_tokens(&["Int".to_string()]),
        }],
        return_ty: None,
        clauses: vec![AstClause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(AstExpr::Call {
                func: Box::new(Spanned::no_span(AstExpr::Ident("loop_forever".into()))),
                args: vec![Spanned::no_span(AstExpr::Ident("n".into()))],
            }),
            effect_variables: vec![],
        }],
        is_ghost: false,
        is_lemma: false,
    };
    let (errors, pending) = checker.check_function_totality(&f, &(0..1));
    assert!(errors.is_empty());
    assert!(pending.is_empty());
}

#[test]
fn totality_recursive_no_decreases_a09001() {
    let checker = TotalityChecker::new();
    let f = AstFnDef {
        name: "rec".into(),
        params: vec![AstParam {
            name: "n".into(),
            ty: assura_parser::ast::try_parse_type_tokens(&["Int".to_string()]),
        }],
        return_ty: None,
        clauses: vec![AstClause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(AstExpr::Call {
                func: Box::new(Spanned::no_span(AstExpr::Ident("rec".into()))),
                args: vec![Spanned::no_span(AstExpr::BinOp {
                    lhs: Box::new(Spanned::no_span(AstExpr::Ident("n".into()))),
                    op: AstBinOp::Sub,
                    rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
                })],
            }),
            effect_variables: vec![],
        }],
        is_ghost: false,
        is_lemma: false,
    };
    let (errors, _) = checker.check_function_totality(&f, &(0..1));
    assert!(!errors.is_empty());
    assert!(errors.iter().any(|e| e.code == "A09001"));
}

#[test]
fn totality_non_recursive_is_total() {
    let checker = TotalityChecker::new();
    let f = AstFnDef {
        name: "add".into(),
        params: vec![],
        return_ty: None,
        clauses: vec![AstClause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(AstExpr::BinOp {
                lhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
                op: AstBinOp::Add,
                rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("2".into())))),
            }),
            effect_variables: vec![],
        }],
        is_ghost: false,
        is_lemma: false,
    };
    let (errors, pending) = checker.check_function_totality(&f, &(0..1));
    assert!(errors.is_empty());
    assert!(pending.is_empty());
}

#[test]
fn totality_decreases_with_nat_param() {
    let checker = TotalityChecker::new();
    let f = AstFnDef {
        name: "count".into(),
        params: vec![AstParam {
            name: "n".into(),
            ty: assura_parser::ast::try_parse_type_tokens(&["Nat".to_string()]),
        }],
        return_ty: None,
        clauses: vec![
            AstClause {
                kind: ClauseKind::Decreases,
                body: Spanned::no_span(AstExpr::Ident("n".into())),
                effect_variables: vec![],
            },
            AstClause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(AstExpr::Call {
                    func: Box::new(Spanned::no_span(AstExpr::Ident("count".into()))),
                    args: vec![Spanned::no_span(AstExpr::BinOp {
                        lhs: Box::new(Spanned::no_span(AstExpr::Ident("n".into()))),
                        op: AstBinOp::Sub,
                        rhs: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
                    })],
                }),
                effect_variables: vec![],
            },
        ],
        is_ghost: false,
        is_lemma: false,
    };
    let (errors, pending) = checker.check_function_totality(&f, &(0..1));
    // With Nat param, well-foundedness is automatically satisfied
    assert!(errors.is_empty(), "errors: {errors:?}");
    assert!(pending.is_empty());
}

#[test]
fn totality_is_partial_from_clause() {
    let checker = TotalityChecker::new();
    let f = AstFnDef {
        name: "diverge".into(),
        params: vec![],
        return_ty: None,
        clauses: vec![AstClause {
            kind: ClauseKind::Other("partial".into()),
            body: Spanned::no_span(AstExpr::Literal(AstLit::Bool(true))),
            effect_variables: vec![],
        }],
        is_ghost: false,
        is_lemma: false,
    };
    assert!(checker.is_partial(&f));
}

// -----------------------------------------------------------------------
// TypestateChecker additional tests
// -----------------------------------------------------------------------

#[test]
fn typestate_self_loop_transition() {
    let states = vec!["Running".into()];
    let transitions = vec![("tick".into(), "Running".into(), "Running".into())];
    let mut checker = TypestateChecker::new(states, transitions, "Running".into(), 0..4);
    checker.transition("tick", 0..1).unwrap();
    assert_eq!(checker.current_state(), "Running");
    checker.transition("tick", 0..1).unwrap();
}

#[test]
fn typestate_empty_transitions_no_errors() {
    let states = vec!["Init".into()];
    let checker = TypestateChecker::new(states, vec![], "Init".into(), 0..4);
    assert!(checker.validate_transitions().is_empty());
}

#[test]
fn typestate_branch_consistency_same_state() {
    let states = vec!["S1".into()];
    let a = TypestateChecker::new(states.clone(), vec![], "S1".into(), 0..1);
    let b = TypestateChecker::new(states, vec![], "S1".into(), 0..1);
    assert!(TypestateChecker::check_branch_consistency(&a, &b, 0..1).is_none());
}

#[test]
fn typestate_validate_linear_true() {
    let states = vec!["S".into()];
    let checker = TypestateChecker::new(states, vec![], "S".into(), 0..1);
    assert!(checker.validate_linear(true).is_none());
}

#[test]
fn typestate_validate_multiple_undeclared_states() {
    let states = vec!["Init".into()];
    let transitions = vec![
        ("a".into(), "X".into(), "Y".into()), // X and Y both undeclared
    ];
    let checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);
    let errors = checker.validate_transitions();
    assert_eq!(errors.len(), 2);
    assert!(errors.iter().all(|e| e.code == "A06003"));
}

// -----------------------------------------------------------------------
// EffectChecker additional tests
// -----------------------------------------------------------------------

#[test]
fn effect_checker_sub_effect_of_known_io() {
    let checker = EffectChecker::new();
    // io.custom should be accepted because "io" is a known group
    let set = EffectSet::from_effect_names(["io.custom"]);
    let errors = checker.check_known(&set, &(0..1));
    assert!(errors.is_empty(), "io.custom is a sub-effect of known io");
}

#[test]
fn effect_checker_capitalized_name_skipped() {
    let checker = EffectChecker::new();
    // Capitalized names are skipped in check_known (they're type names)
    let set = EffectSet::from_effect_names(["InflateDecoder"]);
    let errors = checker.check_known(&set, &(0..1));
    assert!(errors.is_empty(), "capitalized names are skipped");
}

#[test]
fn effect_checker_block_keyword_skipped() {
    let checker = EffectChecker::new();
    // Block-kind keywords like "incremental" should not be flagged
    let set = EffectSet::from_effect_names(["incremental"]);
    let errors = checker.check_known(&set, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn effect_expand_net_includes_network_subeffects() {
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["net"]);
    let expanded = checker.expand(&declared);
    assert!(expanded.contains("network.connect"));
    assert!(expanded.contains("network.send"));
    assert!(expanded.contains("network.receive"));
}

#[test]
fn effect_expand_fs_includes_filesystem_subeffects() {
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["fs"]);
    let expanded = checker.expand(&declared);
    assert!(expanded.contains("filesystem.read"));
    assert!(expanded.contains("filesystem.write"));
}

// -----------------------------------------------------------------------
// InfoFlowChecker additional tests
// -----------------------------------------------------------------------

#[test]
fn info_flow_dc_same_level_assignment_ok() {
    let checker = InfoFlowChecker::new();
    let err = checker.check_assignment(
        SecurityLabel::Confidential,
        SecurityLabel::Confidential,
        &(0..1),
    );
    assert!(err.is_none());
}

#[test]
fn info_flow_dc_upward_ok() {
    let checker = InfoFlowChecker::new();
    let err = checker.check_assignment(SecurityLabel::Restricted, SecurityLabel::Public, &(0..1));
    assert!(err.is_none(), "public to restricted is upward flow, OK");
}

#[test]
fn info_flow_dc_downward_a08001() {
    let checker = InfoFlowChecker::new();
    let err = checker.check_assignment(SecurityLabel::Public, SecurityLabel::Restricted, &(0..1));
    assert_eq!(err.unwrap().code, "A08001");
}

#[test]
fn info_flow_dc_declassify_annotated_ok() {
    let checker = InfoFlowChecker::new();
    let err = checker.check_declassify(
        SecurityLabel::Restricted,
        SecurityLabel::Public,
        true,
        &(0..1),
    );
    assert!(err.is_none());
}

#[test]
fn info_flow_dc_declassify_not_annotated_a08002() {
    let checker = InfoFlowChecker::new();
    let err = checker.check_declassify(
        SecurityLabel::Restricted,
        SecurityLabel::Public,
        false,
        &(0..1),
    );
    assert_eq!(err.unwrap().code, "A08002");
}

#[test]
fn info_flow_dc_purpose_mismatch_a08003() {
    let mut checker = InfoFlowChecker::new();
    checker.declare_purpose("email".into(), "marketing".into());
    let err = checker.check_purpose_label("email", "billing", &(0..1));
    assert_eq!(err.unwrap().code, "A08003");
}

#[test]
fn info_flow_dc_implicit_flow_a08004() {
    let checker = InfoFlowChecker::new();
    let err =
        checker.check_implicit_flow(SecurityLabel::Confidential, SecurityLabel::Public, &(0..1));
    assert_eq!(err.unwrap().code, "A08004");
}

#[test]
fn info_flow_dc_covert_channel_a08005() {
    let checker = InfoFlowChecker::new();
    let err = checker.check_covert_channel(SecurityLabel::Confidential, "sleep", &(0..1));
    assert_eq!(err.unwrap().code, "A08005");
}

#[test]
fn info_flow_dc_covert_channel_public_ok() {
    let checker = InfoFlowChecker::new();
    let err = checker.check_covert_channel(SecurityLabel::Public, "sleep", &(0..1));
    assert!(err.is_none());
}

#[test]
fn info_flow_dc_has_labels() {
    let mut checker = InfoFlowChecker::new();
    assert!(!checker.has_labels());
    checker.declare("x".into(), SecurityLabel::Public);
    assert!(checker.has_labels());
}

// -----------------------------------------------------------------------
// MemoryChecker additional tests
// -----------------------------------------------------------------------

#[test]
fn memory_checker_dc_buffer_names() {
    let mut checker = MemoryChecker::new();
    checker.register_buffer("a".into(), "a.len".into());
    checker.register_buffer("b".into(), "b.len".into());
    let mut names = checker.buffer_names();
    names.sort();
    assert_eq!(names, vec!["a", "b"]);
}

#[test]
fn memory_checker_dc_non_buffer_check_returns_none() {
    let checker = MemoryChecker::new();
    // Checking a non-registered buffer returns None (out of scope)
    let result = checker.check_bounds_in_requires("unregistered", &[], &(0..1));
    assert!(result.is_none());
}

#[test]
fn memory_checker_dc_multiple_regions() {
    let mut checker = MemoryChecker::new();
    checker.register_buffer("buf".into(), "buf.len".into());
    checker.register_region(MemoryRegion {
        name: "r1".into(),
        lower: "0".into(),
        upper: "10".into(),
        buffer: "buf".into(),
    });
    checker.register_region(MemoryRegion {
        name: "r2".into(),
        lower: "10".into(),
        upper: "20".into(),
        buffer: "buf".into(),
    });
    assert_eq!(checker.regions().len(), 2);
    assert!(checker.check_region_buffers(&(0..1)).is_empty());
}

#[test]
fn memory_checker_dc_region_containment_undefined_parent() {
    let mut checker = MemoryChecker::new();
    checker.register_buffer("buf".into(), "buf.len".into());
    checker.register_region(MemoryRegion {
        name: "sub".into(),
        lower: "0".into(),
        upper: "5".into(),
        buffer: "buf".into(),
    });
    let result = checker.check_region_containment("sub", "missing_parent", &(0..1));
    assert_eq!(result.unwrap().code, "A08102");
}

#[test]
fn memory_checker_dc_region_incomplete_bounds() {
    let mut checker = MemoryChecker::new();
    checker.register_buffer("buf".into(), "buf.len".into());
    checker.register_region(MemoryRegion {
        name: "empty_bounds".into(),
        lower: "".into(),
        upper: "".into(),
        buffer: "buf".into(),
    });
    checker.register_region(MemoryRegion {
        name: "parent".into(),
        lower: "0".into(),
        upper: "100".into(),
        buffer: "buf".into(),
    });
    let result = checker.check_region_containment("empty_bounds", "parent", &(0..1));
    assert_eq!(result.unwrap().code, "A08102");
}

// -----------------------------------------------------------------------
// FrameChecker additional tests (via frame module functions)
// -----------------------------------------------------------------------

#[test]
fn frame_dc_extract_modifies_single() {
    let expr = Spanned::no_span(AstExpr::Ident("x".into()));
    let targets = extract_modifies_targets(&expr);
    assert_eq!(targets, vec!["x"]);
}

#[test]
fn frame_dc_extract_modifies_field() {
    let expr = Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("obj".into()))),
        "count".into(),
    ));
    let targets = extract_modifies_targets(&expr);
    assert_eq!(targets, vec!["obj.count"]);
}

#[test]
fn frame_dc_extract_modifies_block_multiple() {
    let expr = Spanned::no_span(AstExpr::Block(vec![
        Spanned::no_span(AstExpr::Ident("a".into())),
        Spanned::no_span(AstExpr::Ident("b".into())),
        Spanned::no_span(AstExpr::Ident("c".into())),
    ]));
    let targets = extract_modifies_targets(&expr);
    assert_eq!(targets, vec!["a", "b", "c"]);
}

#[test]
fn frame_dc_extract_modifies_list() {
    let expr = Spanned::no_span(AstExpr::List(vec![
        Spanned::no_span(AstExpr::Ident("x".into())),
        Spanned::no_span(AstExpr::Ident("y".into())),
    ]));
    let targets = extract_modifies_targets(&expr);
    assert_eq!(targets, vec!["x", "y"]);
}

#[test]
fn frame_dc_extract_modifies_ident() {
    let expr = Spanned::no_span(AstExpr::Ident("z".into()));
    let targets = extract_modifies_targets(&expr);
    assert_eq!(targets, vec!["z"]);
}

#[test]
fn frame_dc_collect_old_references_basic() {
    let expr = Spanned::no_span(AstExpr::Old(Box::new(Spanned::no_span(AstExpr::Ident(
        "x".into(),
    )))));
    let refs = collect_old_references(&expr);
    assert_eq!(refs, vec!["x"]);
}

#[test]
fn frame_dc_collect_old_references_field() {
    let expr = Spanned::no_span(AstExpr::Old(Box::new(Spanned::no_span(AstExpr::Field(
        Box::new(Spanned::no_span(AstExpr::Ident("obj".into()))),
        "val".into(),
    )))));
    let refs = collect_old_references(&expr);
    assert!(refs.contains(&"obj.val".to_string()));
}

#[test]
fn frame_dc_collect_old_references_nested_expr() {
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Old(Box::new(Spanned::no_span(
            AstExpr::Ident("a".into()),
        ))))),
        op: AstBinOp::Add,
        rhs: Box::new(Spanned::no_span(AstExpr::Old(Box::new(Spanned::no_span(
            AstExpr::Ident("b".into()),
        ))))),
    });
    let refs = collect_old_references(&expr);
    assert!(refs.contains(&"a".to_string()));
    assert!(refs.contains(&"b".to_string()));
}

#[test]
fn frame_dc_collect_ident_references() {
    let expr = Spanned::no_span(AstExpr::BinOp {
        lhs: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        op: AstBinOp::Add,
        rhs: Box::new(Spanned::no_span(AstExpr::Ident("y".into()))),
    });
    let refs = collect_ident_references(&expr);
    assert!(refs.contains(&"x".to_string()));
    assert!(refs.contains(&"y".to_string()));
}

#[test]
fn frame_dc_collect_ident_references_skips_keywords() {
    let expr = Spanned::no_span(AstExpr::Ident("result".into()));
    let refs = collect_ident_references(&expr);
    assert!(refs.is_empty(), "result should be skipped");
}

// -----------------------------------------------------------------------
// SecureErasureChecker additional tests
// -----------------------------------------------------------------------

#[test]
fn secure_erasure_dc_sensitive_names() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("key1".into());
    checker.mark_sensitive("key2".into());
    let mut names = checker.sensitive_names();
    names.sort();
    assert_eq!(names, vec!["key1", "key2"]);
}

#[test]
fn secure_erasure_dc_return_sensitive_ok() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("derived".into());
    let errors = checker.check_return("derived", true, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn secure_erasure_dc_copy_non_sensitive_source_ok() {
    let checker = SecureErasureChecker::new();
    // Source is not sensitive, so copy is fine
    let errors = checker.check_copy("public_data", "dest", false, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn secure_erasure_dc_check_all_erased_all_zeroized() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("k1".into());
    checker.mark_sensitive("k2".into());
    checker.mark_zeroized("k1".into());
    checker.mark_zeroized("k2".into());
    let errors = checker.check_all_erased(&(0..1));
    assert!(errors.is_empty());
}

#[test]
fn secure_erasure_dc_default_empty() {
    let checker = SecureErasureChecker::default();
    assert!(checker.sensitive_names().is_empty());
    assert!(checker.check_all_erased(&(0..1)).is_empty());
}

// -----------------------------------------------------------------------
// ConstantTimeChecker additional tests
// -----------------------------------------------------------------------

#[test]
fn ct_dc_check_expr_index_with_secret() {
    let mut checker = ConstantTimeChecker::new();
    checker.mark_secret("key_byte".into());
    let expr = Spanned::no_span(AstExpr::Index {
        expr: Box::new(Spanned::no_span(AstExpr::Ident("table".into()))),
        index: Box::new(Spanned::no_span(AstExpr::Ident("key_byte".into()))),
    });
    let errors = checker.check_expr(&expr, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A14002");
}

#[test]
fn ct_dc_check_expr_nested_if() {
    let mut checker = ConstantTimeChecker::new();
    checker.mark_secret("s".into());
    let expr = Spanned::no_span(AstExpr::If {
        cond: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Bool(true)))),
        then_branch: Box::new(Spanned::no_span(AstExpr::If {
            cond: Box::new(Spanned::no_span(AstExpr::Ident("s".into()))),
            then_branch: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
            else_branch: None,
        })),
        else_branch: None,
    });
    let errors = checker.check_expr(&expr, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A14001");
}

#[test]
fn ct_dc_no_secrets_no_errors() {
    let checker = ConstantTimeChecker::new();
    let expr = Spanned::no_span(AstExpr::If {
        cond: Box::new(Spanned::no_span(AstExpr::Ident("x".into()))),
        then_branch: Box::new(Spanned::no_span(AstExpr::Literal(AstLit::Int("1".into())))),
        else_branch: None,
    });
    let errors = checker.check_expr(&expr, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn ct_dc_references_secret_through_call() {
    let mut checker = ConstantTimeChecker::new();
    checker.mark_secret("hmac_key".into());
    let expr = Spanned::no_span(AstExpr::Call {
        func: Box::new(Spanned::no_span(AstExpr::Ident("compute".into()))),
        args: vec![Spanned::no_span(AstExpr::Ident("hmac_key".into()))],
    });
    assert!(checker.references_secret(&expr));
}

#[test]
fn ct_dc_references_secret_through_index() {
    let mut checker = ConstantTimeChecker::new();
    checker.mark_secret("idx".into());
    let expr = Spanned::no_span(AstExpr::Index {
        expr: Box::new(Spanned::no_span(AstExpr::Ident("table".into()))),
        index: Box::new(Spanned::no_span(AstExpr::Ident("idx".into()))),
    });
    assert!(checker.references_secret(&expr));
}

// -----------------------------------------------------------------------
// DeterminismChecker additional tests
// -----------------------------------------------------------------------

#[test]
fn determinism_dc_is_non_deterministic() {
    let checker = DeterminismChecker::new();
    assert!(checker.is_non_deterministic("HashMap"));
    assert!(checker.is_non_deterministic("HashSet"));
    assert!(checker.is_non_deterministic("random"));
    assert!(checker.is_non_deterministic("thread_rng"));
    assert!(!checker.is_non_deterministic("Vec"));
    assert!(!checker.is_non_deterministic("BTreeMap"));
}

#[test]
fn determinism_dc_custom_source() {
    let mut checker = DeterminismChecker::new();
    checker.add_non_det_source("UuidV4::new".into());
    assert!(checker.is_non_deterministic("UuidV4::new"));
}

#[test]
fn determinism_dc_non_det_fn_skips_check() {
    let checker = DeterminismChecker::new();
    // Not marked deterministic -> no errors even with random
    let errors = checker.check_fn_body("my_fn", &["random".into()], &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn determinism_dc_multiple_violations() {
    let mut checker = DeterminismChecker::new();
    checker.mark_deterministic("pure_fn".into());
    let errors = checker.check_fn_body(
        "pure_fn",
        &["HashMap".into(), "HashSet".into(), "random".into()],
        &(0..1),
    );
    assert_eq!(errors.len(), 3);
    assert!(errors.iter().all(|e| e.code == "A20001"));
}

#[test]
fn determinism_dc_hashset_iteration_a20002() {
    let mut checker = DeterminismChecker::new();
    checker.mark_deterministic("process".into());
    let errors = checker.check_iteration("process", "HashSet<i32>", &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A20002");
}

// -----------------------------------------------------------------------
// FfiBoundaryChecker additional tests
// -----------------------------------------------------------------------

#[test]
fn ffi_dc_audited_no_contract_ok() {
    let mut checker = FfiBoundaryChecker::new();
    checker.register_extern("audited_fn".into(), TrustBoundary::Audited);
    let errors = checker.check_extern_decl("audited_fn", true, false, &(0..1));
    assert!(errors.is_empty(), "audited extern doesn't need a contract");
}

#[test]
fn ffi_dc_call_contracted_untrusted_ok() {
    let mut checker = FfiBoundaryChecker::new();
    checker.register_extern("ffi_read".into(), TrustBoundary::Untrusted);
    checker.mark_contracted("ffi_read".into());
    // Contracted FFI call skips validation check
    let errors = checker.check_ffi_call("ffi_read", false, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn ffi_dc_unsafe_no_unsafe_ok() {
    let checker = FfiBoundaryChecker::new();
    let errors = checker.check_unsafe_confinement("pure_fn", false, false, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn ffi_dc_call_unknown_extern_ok() {
    let checker = FfiBoundaryChecker::new();
    // Calling an unregistered extern is fine (it's not untrusted)
    let errors = checker.check_ffi_call("unknown_fn", false, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn ffi_dc_default_empty() {
    let checker = FfiBoundaryChecker::default();
    let errors = checker.check_extern_decl("x", true, true, &(0..1));
    assert!(errors.is_empty());
}

// -----------------------------------------------------------------------
// LockOrderChecker additional tests
// -----------------------------------------------------------------------

#[test]
fn lock_order_dc_single_lock_ok() {
    let mut checker = LockOrderChecker::new();
    checker.define_order("mutex".into(), 1);
    let errors = checker.acquire("mutex", &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn lock_order_dc_release_reverse_ok() {
    let mut checker = LockOrderChecker::new();
    checker.define_order("a".into(), 1);
    checker.define_order("b".into(), 2);
    checker.acquire("a", &(0..1));
    checker.acquire("b", &(0..1));
    // Release in reverse order: b first, then a
    let errors = checker.release("b", &(0..1));
    assert!(errors.is_empty());
    let errors = checker.release("a", &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn lock_order_dc_release_wrong_order_a21002() {
    let mut checker = LockOrderChecker::new();
    checker.define_order("a".into(), 1);
    checker.define_order("b".into(), 2);
    checker.acquire("a", &(0..1));
    checker.acquire("b", &(0..1));
    // Release a before b -> wrong order
    let errors = checker.release("a", &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A21002");
}

#[test]
fn lock_order_dc_undefined_a21003() {
    let checker = LockOrderChecker::new();
    let errors = checker.check_ordering_defined("unknown_lock", &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A21003");
}

#[test]
fn lock_order_dc_defined_ok() {
    let mut checker = LockOrderChecker::new();
    checker.define_order("known_lock".into(), 5);
    let errors = checker.check_ordering_defined("known_lock", &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn lock_order_dc_same_priority_violation() {
    let mut checker = LockOrderChecker::new();
    checker.define_order("lock_a".into(), 1);
    checker.define_order("lock_b".into(), 1);
    checker.acquire("lock_a", &(0..1));
    let errors = checker.acquire("lock_b", &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A21001");
}

// -----------------------------------------------------------------------
// SharedMemChecker additional tests
// -----------------------------------------------------------------------

#[test]
fn shared_mem_dc_write_none_a18002() {
    let checker = SharedMemChecker::new();
    let errors = checker.check_write("buffer", &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A18002");
}

#[test]
fn shared_mem_dc_read_unregistered_a18001() {
    let checker = SharedMemChecker::new();
    let errors = checker.check_read("unregistered", &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A18001");
}

#[test]
fn shared_mem_dc_data_race_both_exclusive_a18003() {
    let checker = SharedMemChecker::new();
    let errors =
        checker.check_data_race("obj", AccessMode::Exclusive, AccessMode::Exclusive, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A18003");
}

#[test]
fn shared_mem_dc_no_race_both_none() {
    let checker = SharedMemChecker::new();
    let errors = checker.check_data_race("obj", AccessMode::None, AccessMode::None, &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn shared_mem_dc_set_mode_overwrite() {
    let mut checker = SharedMemChecker::new();
    checker.set_mode("buf".into(), AccessMode::None);
    let errors = checker.check_read("buf", &(0..1));
    assert_eq!(errors.len(), 1);
    // Upgrade to exclusive
    checker.set_mode("buf".into(), AccessMode::Exclusive);
    let errors = checker.check_read("buf", &(0..1));
    assert!(errors.is_empty());
}

// -----------------------------------------------------------------------
// CrashRecoveryChecker additional tests
// -----------------------------------------------------------------------

#[test]
fn crash_recovery_dc_multiple_txns() {
    let mut cr = CrashRecoveryChecker::new();
    cr.begin_write("t1".into());
    cr.begin_write("t2".into());
    cr.write_wal("t1");
    cr.write_data("t1");
    cr.fsync("t1");
    cr.commit("t1");
    // t2 still has no WAL
    cr.write_data("t2");
    let errs = cr.check_write_ahead();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A33001");
}

#[test]
fn crash_recovery_dc_commit_with_fsync_ok() {
    let mut cr = CrashRecoveryChecker::new();
    cr.begin_write("t1".into());
    cr.write_wal("t1");
    cr.write_data("t1");
    cr.fsync("t1");
    cr.commit("t1");
    assert!(cr.check_commit_durability().is_empty());
}

#[test]
fn crash_recovery_dc_ordering_correct() {
    let mut cr = CrashRecoveryChecker::new();
    cr.begin_write("t1".into());
    cr.write_wal("t1");
    cr.write_data("t1");
    cr.fsync("t1");
    assert!(cr.check_ordering().is_empty());
}

#[test]
fn crash_recovery_dc_no_txns_check_all_ok() {
    let cr = CrashRecoveryChecker::new();
    assert!(cr.check_all().is_empty());
}

#[test]
fn crash_recovery_dc_wal_then_data_then_fsync_ok() {
    let mut cr = CrashRecoveryChecker::new();
    cr.begin_write("tx".into());
    cr.write_wal("tx");
    cr.write_data("tx");
    cr.fsync("tx");
    assert!(cr.check_ordering().is_empty());
    assert!(cr.check_write_ahead().is_empty());
}

// =======================================================================
// Missing domain checker error code coverage (#174)
// =======================================================================

// FfiBoundaryChecker: extern without contract triggers A11001
#[test]
fn ffi_dc_no_contract_a11001() {
    let checker = FfiBoundaryChecker::new();
    let errors = checker.check_extern_decl("malloc", false, false, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A11001");
}

// ErrorPropagationChecker: swallowing must_propagate error triggers A12001
#[test]
fn error_propagation_dc_swallow_a12001() {
    let mut checker = ErrorPropagationChecker::new();
    checker.register_policy(
        "TestPolicy".into(),
        ErrorPolicy {
            must_propagate: vec!["CRITICAL_ERROR".into()],
            ..Default::default()
        },
    );
    let err = checker.validate_catch("CRITICAL_ERROR", ErrorAction::Swallow, 0..10);
    assert!(err.is_some(), "swallowing must_propagate error should fail");
    assert_eq!(err.unwrap().code, "A12001");
}

// InterfaceChecker: missing method triggers A13001
#[test]
fn interface_dc_missing_method_a13001() {
    let mut checker = InterfaceChecker::new();
    checker.register_interface(InterfaceContract {
        name: "Serializable".into(),
        methods: vec![
            InterfaceMethod {
                name: "serialize".into(),
                param_types: vec![],
                return_type: Type::Bytes,
                has_requires: false,
                has_ensures: true,
                no_reentrancy: false,
            },
            InterfaceMethod {
                name: "deserialize".into(),
                param_types: vec![Type::Bytes],
                return_type: Type::Bool,
                has_requires: false,
                has_ensures: true,
                no_reentrancy: false,
            },
        ],
        extends: vec![],
    });
    // Only implement serialize, missing deserialize
    let errors = checker.check_impl("MyType", "Serializable", &["serialize".into()], &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A13001");
}

// SecureErasureChecker: non-zeroized sensitive data triggers A16001
#[test]
fn secure_erasure_dc_not_zeroized_a16001() {
    let mut checker = SecureErasureChecker::new();
    checker.mark_sensitive("private_key".into());
    let errors = checker.check_scope_exit("private_key", &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A16001");
}

// CryptoConformanceChecker: wrong key size triggers A17001
#[test]
fn crypto_dc_wrong_key_size_a17001() {
    let checker = CryptoConformanceChecker::new();
    let errors = checker.check_key_size("AES-128-GCM", 256, &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A17001");
}
