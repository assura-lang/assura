use super::*;

#[test]
fn test_register_builtin_measures_count() {
    let measures = register_builtin_measures();
    assert_eq!(measures.len(), 5, "should have 5 built-in measures");
}

#[test]
fn test_builtin_measure_names() {
    let measures = register_builtin_measures();
    let names: Vec<&str> = measures.iter().map(|m| m.name.as_str()).collect();
    assert!(names.contains(&"len"), "should contain len");
    assert!(names.contains(&"elems"), "should contain elems");
    assert!(names.contains(&"keys"), "should contain keys");
    assert!(names.contains(&"values"), "should contain values");
    assert!(names.contains(&"size"), "should contain size");
}

#[test]
fn test_len_measure_properties() {
    let measures = register_builtin_measures();
    let len = measures.iter().find(|m| m.name == "len").unwrap();
    assert_eq!(len.param_sorts, vec![MeasureSort::Collection]);
    assert_eq!(len.return_sort, MeasureSort::Nat);
    assert!(len.returns_nat());
    assert_eq!(len.axioms.len(), 3, "len should have 3 axioms");
}

#[test]
fn test_elems_measure_returns_set() {
    let measures = register_builtin_measures();
    let elems = measures.iter().find(|m| m.name == "elems").unwrap();
    assert_eq!(elems.return_sort, MeasureSort::Set);
    assert!(!elems.returns_nat());
}

#[test]
fn test_keys_measure_takes_map() {
    let measures = register_builtin_measures();
    let keys = measures.iter().find(|m| m.name == "keys").unwrap();
    assert_eq!(keys.param_sorts, vec![MeasureSort::Map]);
    assert_eq!(keys.return_sort, MeasureSort::Set);
}

#[test]
fn test_values_measure_takes_map() {
    let measures = register_builtin_measures();
    let values = measures.iter().find(|m| m.name == "values").unwrap();
    assert_eq!(values.param_sorts, vec![MeasureSort::Map]);
    assert_eq!(values.return_sort, MeasureSort::Set);
}

#[test]
fn test_size_measure_has_equivalence_axiom() {
    let measures = register_builtin_measures();
    let size = measures.iter().find(|m| m.name == "size").unwrap();
    assert!(size.returns_nat());
    let has_equiv = size
        .axioms
        .iter()
        .any(|a| matches!(&a.tag, MeasureAxiomTag::EquivalentTo(name) if name == "len"));
    assert!(has_equiv, "size should have EquivalentTo(len) axiom");
}

#[test]
fn test_measure_definition_builder() {
    let m = MeasureDefinition::new("custom", vec![MeasureSort::Collection], MeasureSort::Nat)
        .with_axiom("custom(x) >= 0", MeasureAxiomTag::NonNegative)
        .with_axiom("custom note", MeasureAxiomTag::Custom("note".into()));

    assert_eq!(m.name, "custom");
    assert_eq!(m.axioms.len(), 2);
    assert_eq!(m.axioms[0].description, "custom(x) >= 0");
    assert!(matches!(m.axioms[0].tag, MeasureAxiomTag::NonNegative));
    assert!(matches!(&m.axioms[1].tag, MeasureAxiomTag::Custom(s) if s == "note"));
}

#[test]
fn test_measure_sort_equality() {
    assert_eq!(MeasureSort::Nat, MeasureSort::Nat);
    assert_ne!(MeasureSort::Nat, MeasureSort::Set);
    assert_ne!(MeasureSort::Collection, MeasureSort::Map);
}

#[test]
fn test_len_axiom_tags() {
    let measures = register_builtin_measures();
    let len = measures.iter().find(|m| m.name == "len").unwrap();
    let tags: Vec<&MeasureAxiomTag> = len.axioms.iter().map(|a| &a.tag).collect();
    assert!(
        tags.contains(&&MeasureAxiomTag::NonNegative),
        "len should have NonNegative axiom"
    );
    assert!(
        tags.contains(&&MeasureAxiomTag::EmptyIsZero),
        "len should have EmptyIsZero axiom"
    );
    assert!(
        tags.contains(&&MeasureAxiomTag::AppendIncrement),
        "len should have AppendIncrement axiom"
    );
}

// =======================================================================
// T076: Layer 2 SMT encoding tests
// =======================================================================

#[test]
fn layer2_config_default() {
    let config = Layer2Config::default();
    assert_eq!(config.timeout_ms, 10_000);
    assert!(config.enable_quantifiers);
    assert!(config.enable_termination);
    assert!(config.enable_roundtrip);
}

#[test]
fn layer2_config_custom_timeout() {
    let config = Layer2Config::new().with_timeout(5_000);
    assert_eq!(config.timeout_ms, 5_000);
}

#[test]
fn layer2_verifier_add_invariant() {
    let mut verifier = Layer2Verifier::new(Layer2Config::default());
    verifier.add_invariant(QuantifiedInvariant {
        name: "sorted_inv".into(),
        bound_vars: vec![("i".into(), "Int".into()), ("j".into(), "Int".into())],
        body: "i < j => a[i] <= a[j]".into(),
        triggers: vec!["a[i]".into(), "a[j]".into()],
    });
    assert_eq!(verifier.obligation_count(), 1);
}

#[test]
fn layer2_verifier_structural_check() {
    let mut verifier = Layer2Verifier::new(Layer2Config::default());
    verifier.add_invariant(QuantifiedInvariant {
        name: "inv1".into(),
        bound_vars: vec![("x".into(), "Int".into())],
        body: "f(x) >= 0".into(),
        triggers: vec![],
    });
    verifier.add_termination(TerminationObligation {
        fn_name: "fib".into(),
        measure: "n".into(),
        recursive_calls: vec!["fib(n-1)".into(), "fib(n-2)".into()],
    });
    verifier.add_roundtrip(RoundtripObligation {
        type_name: "Message".into(),
        serialize_fn: "encode".into(),
        deserialize_fn: "decode".into(),
    });
    let results = verifier.check_structural();
    assert_eq!(results.len(), 3);
    // check_structural returns Unknown (not Verified) because Z3 is not used
    assert!(
        matches!(&results[0], Layer2Result::Unknown { invariant, reason } if invariant == "inv1" && reason.contains("structural pre-check"))
    );
    assert!(
        matches!(&results[1], Layer2Result::Unknown { invariant, reason } if invariant == "termination:fib" && reason.contains("structural pre-check"))
    );
    assert!(
        matches!(&results[2], Layer2Result::Unknown { invariant, reason } if invariant == "roundtrip:Message" && reason.contains("structural pre-check"))
    );
}

#[test]
fn layer2_empty_bound_vars() {
    let mut verifier = Layer2Verifier::new(Layer2Config::default());
    verifier.add_invariant(QuantifiedInvariant {
        name: "bad_inv".into(),
        bound_vars: vec![],
        body: "true".into(),
        triggers: vec![],
    });
    let results = verifier.check_structural();
    assert!(
        matches!(&results[0], Layer2Result::Unknown { reason, .. } if reason.contains("no bound variables"))
    );
}

#[test]
fn layer2_no_measure() {
    let mut verifier = Layer2Verifier::new(Layer2Config::default());
    verifier.add_termination(TerminationObligation {
        fn_name: "loop".into(),
        measure: String::new(),
        recursive_calls: vec![],
    });
    let results = verifier.check_structural();
    assert!(
        matches!(&results[0], Layer2Result::Unknown { reason, .. } if reason.contains("no measure"))
    );
}

#[test]
fn layer2_obligation_count() {
    let mut verifier = Layer2Verifier::new(Layer2Config::default());
    assert_eq!(verifier.obligation_count(), 0);
    verifier.add_invariant(QuantifiedInvariant {
        name: "a".into(),
        bound_vars: vec![("x".into(), "Int".into())],
        body: "true".into(),
        triggers: vec![],
    });
    verifier.add_termination(TerminationObligation {
        fn_name: "f".into(),
        measure: "n".into(),
        recursive_calls: vec![],
    });
    assert_eq!(verifier.obligation_count(), 2);
}

// =======================================================================
// T078: Quantifier trigger tests
// =======================================================================

#[test]
fn trigger_infer_from_known_fn() {
    let mut mgr = TriggerManager::new();
    mgr.register_function("len".into());
    let trigger = mgr.infer_trigger("len(xs) >= 0");
    assert!(trigger.is_some());
    assert!(!trigger.unwrap().is_user_provided);
}

#[test]
fn trigger_infer_no_match() {
    let mgr = TriggerManager::new();
    let trigger = mgr.infer_trigger("x + y > 0");
    assert!(trigger.is_none());
}

#[test]
fn trigger_validate_known() {
    let mut mgr = TriggerManager::new();
    mgr.register_function("f".into());
    let pattern = TriggerPattern {
        terms: vec!["f(x)".into()],
        is_user_provided: true,
    };
    let warnings = mgr.validate_trigger(&pattern);
    assert!(warnings.is_empty());
}

#[test]
fn trigger_validate_unknown() {
    let mgr = TriggerManager::new();
    let pattern = TriggerPattern {
        terms: vec!["unknown(x)".into()],
        is_user_provided: true,
    };
    let warnings = mgr.validate_trigger(&pattern);
    assert_eq!(warnings.len(), 1);
}

#[test]
fn trigger_add_and_get() {
    let mut mgr = TriggerManager::new();
    mgr.add_trigger(
        "forall_sorted".into(),
        TriggerPattern {
            terms: vec!["a[i]".into()],
            is_user_provided: true,
        },
    );
    assert!(mgr.get_triggers("forall_sorted").is_some());
    assert_eq!(mgr.get_triggers("forall_sorted").unwrap().len(), 1);
    assert!(mgr.get_triggers("other").is_none());
}

#[test]
fn trigger_default() {
    let mgr = TriggerManager::default();
    assert!(mgr.get_triggers("x").is_none());
}

// =======================================================================
// T073: Codec dispatch tests
// =======================================================================

#[test]
fn codec_dispatch_match() {
    let mut disp = CodecDispatcher::new();
    disp.register("PNG".into(), vec![0x89, 0x50, 0x4E, 0x47], 0);
    let data = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A];
    assert_eq!(disp.dispatch(&data), DispatchResult::Matched("PNG".into()));
}

#[test]
fn codec_dispatch_unknown() {
    let mut disp = CodecDispatcher::new();
    disp.register("PNG".into(), vec![0x89, 0x50, 0x4E, 0x47], 0);
    let data = vec![0xFF, 0xD8, 0xFF]; // JPEG magic
    assert_eq!(disp.dispatch(&data), DispatchResult::Unknown);
}

#[test]
fn codec_dispatch_ambiguous() {
    let mut disp = CodecDispatcher::new();
    disp.register("FormatA".into(), vec![0x00, 0x01], 0);
    disp.register("FormatB".into(), vec![0x00, 0x01], 0);
    let data = vec![0x00, 0x01, 0x02];
    assert!(matches!(disp.dispatch(&data), DispatchResult::Ambiguous(_)));
}

#[test]
fn codec_dispatch_offset() {
    let mut disp = CodecDispatcher::new();
    disp.register("ZIP".into(), vec![0x50, 0x4B], 0);
    disp.register("DocX".into(), vec![0x50, 0x4B, 0x03, 0x04], 0);
    // Both match the same prefix
    let data = vec![0x50, 0x4B, 0x03, 0x04, 0x00];
    let result = disp.dispatch(&data);
    assert!(matches!(result, DispatchResult::Ambiguous(_)));
}

#[test]
fn codec_check_ambiguity() {
    let mut disp = CodecDispatcher::new();
    disp.register("A".into(), vec![0xFF], 0);
    disp.register("B".into(), vec![0xFF], 0);
    let conflicts = disp.check_ambiguity();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0], ("A".into(), "B".into()));
}

#[test]
fn codec_no_ambiguity() {
    let mut disp = CodecDispatcher::new();
    disp.register("A".into(), vec![0x01], 0);
    disp.register("B".into(), vec![0x02], 0);
    assert!(disp.check_ambiguity().is_empty());
}

#[test]
fn codec_count() {
    let mut disp = CodecDispatcher::new();
    assert_eq!(disp.codec_count(), 0);
    disp.register("X".into(), vec![0x00], 0);
    assert_eq!(disp.codec_count(), 1);
}

#[test]
fn codec_default() {
    let disp = CodecDispatcher::default();
    assert_eq!(disp.codec_count(), 0);
}

#[test]
fn codec_short_data() {
    let mut disp = CodecDispatcher::new();
    disp.register("Long".into(), vec![0x01, 0x02, 0x03, 0x04], 0);
    let data = vec![0x01, 0x02]; // too short
    assert_eq!(disp.dispatch(&data), DispatchResult::Unknown);
}

// =======================================================================
// T092: WeakMemoryChecker tests
// =======================================================================

#[test]
fn weak_memory_data_race() {
    let mut wm = WeakMemoryChecker::new();
    wm.record_access(1, "x".into(), true, MemoryOrdering::Relaxed);
    wm.record_access(2, "x".into(), false, MemoryOrdering::Relaxed);
    let races = wm.check_data_races();
    assert_eq!(races.len(), 1);
    assert!(races[0].contains("data race"));
}

#[test]
fn weak_memory_no_race_with_hb() {
    let mut wm = WeakMemoryChecker::new();
    let s1 = wm.record_access(1, "x".into(), true, MemoryOrdering::Release);
    let s2 = wm.record_access(2, "x".into(), false, MemoryOrdering::Acquire);
    wm.add_happens_before(s1, s2);
    assert!(wm.check_data_races().is_empty());
}

#[test]
fn weak_memory_release_no_acquire() {
    let mut wm = WeakMemoryChecker::new();
    wm.record_access(1, "flag".into(), true, MemoryOrdering::Release);
    let warnings = wm.check_release_acquire();
    assert_eq!(warnings.len(), 1);
}

#[test]
fn weak_memory_relaxed_warning() {
    let mut wm = WeakMemoryChecker::new();
    wm.record_access(1, "shared".into(), true, MemoryOrdering::Relaxed);
    wm.record_access(2, "shared".into(), false, MemoryOrdering::Relaxed);
    let warnings = wm.check_ordering_strength();
    assert_eq!(warnings.len(), 1);
}

#[test]
fn weak_memory_same_thread_ok() {
    let mut wm = WeakMemoryChecker::new();
    wm.record_access(1, "x".into(), true, MemoryOrdering::Relaxed);
    wm.record_access(1, "x".into(), false, MemoryOrdering::Relaxed);
    assert!(wm.check_data_races().is_empty());
}

#[test]
fn weak_memory_default() {
    let wm = WeakMemoryChecker::default();
    assert_eq!(wm.access_count(), 0);
}

// =======================================================================
// T093: ProphecyManager tests
// =======================================================================

#[test]
fn prophecy_unresolved() {
    let mut pm = ProphecyManager::new();
    pm.declare("future_val".into());
    let errs = pm.check_all_resolved();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A05025");
    assert!(errs[0].message.contains("never resolved"));
    assert_eq!(errs[0].variable, "future_val");
}

#[test]
fn prophecy_resolved_ok() {
    let mut pm = ProphecyManager::new();
    pm.declare("future_val".into());
    pm.resolve("future_val", "42".into()).unwrap();
    assert!(pm.check_all_resolved().is_empty());
}

#[test]
fn prophecy_double_resolve() {
    let mut pm = ProphecyManager::new();
    pm.declare("pv".into());
    pm.resolve("pv", "1".into()).unwrap();
    let err = pm.resolve("pv", "2".into());
    assert!(err.is_err());
}

#[test]
fn prophecy_unconstrained() {
    let mut pm = ProphecyManager::new();
    pm.declare("pv".into());
    let errs = pm.check_unconstrained();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A05026");
    assert!(errs[0].message.contains("no constraints"));
    assert_eq!(errs[0].variable, "pv");
}

#[test]
fn prophecy_with_constraints() {
    let mut pm = ProphecyManager::new();
    pm.declare("pv".into());
    pm.add_constraint("pv", "pv > 0".into());
    assert!(pm.check_unconstrained().is_empty());
}

#[test]
fn prophecy_default() {
    let pm = ProphecyManager::default();
    assert_eq!(pm.variable_count(), 0);
}

// =======================================================================
// T094: LivenessChecker tests
// =======================================================================

#[test]
fn liveness_unverified() {
    let mut lc = LivenessChecker::new();
    lc.add_obligation(
        "progress".into(),
        LivenessKind::Eventually,
        "true".into(),
        "done".into(),
    );
    let errs = lc.check_unverified();
    assert_eq!(errs.len(), 1);
}

#[test]
fn liveness_verified_ok() {
    let mut lc = LivenessChecker::new();
    lc.add_obligation(
        "progress".into(),
        LivenessKind::Eventually,
        "true".into(),
        "done".into(),
    );
    lc.mark_verified("progress");
    assert!(lc.check_unverified().is_empty());
}

#[test]
fn liveness_zero_bound() {
    let mut lc = LivenessChecker::new();
    lc.add_obligation(
        "deadline".into(),
        LivenessKind::EventuallyWithin(0),
        "start".into(),
        "end".into(),
    );
    let errs = lc.check_bounded();
    assert_eq!(errs.len(), 1);
}

#[test]
fn liveness_no_fairness() {
    let mut lc = LivenessChecker::new();
    lc.add_obligation(
        "l2r".into(),
        LivenessKind::LeadsTo,
        "req".into(),
        "resp".into(),
    );
    let errs = lc.check_fairness();
    assert_eq!(errs.len(), 1);
}

#[test]
fn liveness_with_fairness_ok() {
    let mut lc = LivenessChecker::new();
    lc.add_obligation(
        "l2r".into(),
        LivenessKind::LeadsTo,
        "req".into(),
        "resp".into(),
    );
    lc.add_fairness("scheduler_fair".into());
    assert!(lc.check_fairness().is_empty());
}

#[test]
fn liveness_default() {
    let lc = LivenessChecker::default();
    assert_eq!(lc.obligation_count(), 0);
}

// =======================================================================
// T112: IrParser tests
// =======================================================================

#[test]
fn ir_parse_fn_decl() {
    let mut parser = IrParser::new();
    parser.parse_text("fn main()").unwrap();
    assert_eq!(parser.node_count(), 1);
}

#[test]
fn ir_parse_var_decl() {
    let mut parser = IrParser::new();
    parser.parse_text("let x: Int").unwrap();
    assert_eq!(parser.node_count(), 1);
}

#[test]
fn ir_parse_return() {
    let mut parser = IrParser::new();
    parser.parse_text("return 42").unwrap();
    assert_eq!(parser.node_count(), 1);
}

#[test]
fn ir_skip_comments() {
    let mut parser = IrParser::new();
    parser.parse_text("// comment\nfn main()").unwrap();
    assert_eq!(parser.node_count(), 1);
}

#[test]
fn ir_serialize() {
    let mut parser = IrParser::new();
    parser.parse_text("fn test()").unwrap();
    let bytes = parser.serialize_binary();
    assert!(!bytes.is_empty());
}

#[test]
fn ir_default() {
    let parser = IrParser::default();
    assert_eq!(parser.node_count(), 0);
}

// =======================================================================
// T113: SessionCache tests (in-memory per-session dedup)
// =======================================================================

#[test]
fn session_cache_hit() {
    let mut cache = SessionCache::new();
    cache.insert("abc123".into(), "verified".into(), 1000);
    assert!(cache.lookup("abc123").is_some());
    assert_eq!(cache.hit_rate(), 1.0);
}

#[test]
fn session_cache_miss() {
    let mut cache = SessionCache::new();
    assert!(cache.lookup("unknown").is_none());
    assert_eq!(cache.hit_rate(), 0.0);
}

#[test]
fn session_cache_invalidate() {
    let mut cache = SessionCache::new();
    cache.insert("abc".into(), "ok".into(), 1);
    cache.invalidate("abc");
    assert!(cache.lookup("abc").is_none());
}

#[test]
fn session_cache_clear() {
    let mut cache = SessionCache::new();
    cache.insert("a".into(), "ok".into(), 1);
    cache.insert("b".into(), "ok".into(), 1);
    cache.clear();
    assert_eq!(cache.entry_count(), 0);
}

#[test]
fn session_cache_default() {
    let cache = SessionCache::default();
    assert_eq!(cache.entry_count(), 0);
}

// =======================================================================
// P006: Filesystem VerificationCache tests
// =======================================================================

#[test]
fn fs_cache_put_and_get() {
    let dir = std::env::temp_dir().join("assura-test-cache-put-get");
    let _ = std::fs::remove_dir_all(&dir);
    let cache = VerificationCache::new(&dir);
    let clauses = vec![assura_parser::ast::Clause {
        kind: assura_parser::ast::ClauseKind::Ensures,
        body: assura_parser::ast::Spanned::no_span(assura_parser::ast::Expr::Ident(
            "result".into(),
        )),
        effect_variables: vec![],
    }];
    let results = vec![VerificationResult::verified("test.ensures")];
    cache.put("test", &clauses, &results);
    let cached = cache.get("test", &clauses);
    assert!(cached.is_some());
    assert_eq!(cached.unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fs_cache_miss_on_different_clauses() {
    let dir = std::env::temp_dir().join("assura-test-cache-miss");
    let _ = std::fs::remove_dir_all(&dir);
    let cache = VerificationCache::new(&dir);
    let clauses_a = vec![assura_parser::ast::Clause {
        kind: assura_parser::ast::ClauseKind::Ensures,
        body: assura_parser::ast::Spanned::no_span(assura_parser::ast::Expr::Ident(
            "result".into(),
        )),
        effect_variables: vec![],
    }];
    let clauses_b = vec![assura_parser::ast::Clause {
        kind: assura_parser::ast::ClauseKind::Requires,
        body: assura_parser::ast::Spanned::no_span(assura_parser::ast::Expr::Ident(
            "result".into(),
        )),
        effect_variables: vec![],
    }];
    let results = vec![VerificationResult::verified("test.ensures")];
    cache.put("test", &clauses_a, &results);
    assert!(cache.get("test", &clauses_b).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fs_cache_clear() {
    let dir = std::env::temp_dir().join("assura-test-cache-clear");
    let _ = std::fs::remove_dir_all(&dir);
    let cache = VerificationCache::new(&dir);
    let clauses = vec![assura_parser::ast::Clause {
        kind: assura_parser::ast::ClauseKind::Ensures,
        body: assura_parser::ast::Spanned::no_span(assura_parser::ast::Expr::Ident(
            "result".into(),
        )),
        effect_variables: vec![],
    }];
    let results = vec![VerificationResult::verified("test.ensures")];
    cache.put("test", &clauses, &results);
    assert_eq!(cache.entry_count(), 1);
    cache.clear();
    assert_eq!(cache.entry_count(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fs_cache_entry_count() {
    let dir = std::env::temp_dir().join("assura-test-cache-count");
    let _ = std::fs::remove_dir_all(&dir);
    let cache = VerificationCache::new(&dir);
    assert_eq!(cache.entry_count(), 0);
    let clauses = vec![assura_parser::ast::Clause {
        kind: assura_parser::ast::ClauseKind::Ensures,
        body: assura_parser::ast::Spanned::no_span(assura_parser::ast::Expr::Ident(
            "result".into(),
        )),
        effect_variables: vec![],
    }];
    cache.put("alpha", &clauses, &[]);
    cache.put("beta", &clauses, &[]);
    assert_eq!(cache.entry_count(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

// =======================================================================
// has_verifiable_clauses tests
// =======================================================================

#[test]
fn has_verifiable_clauses_true_for_requires() {
    let src = "contract Foo { requires x > 0 }";
    let file = assura_parser::parse_unwrap(src);
    assert!(has_verifiable_clauses(&file));
}

#[test]
fn has_verifiable_clauses_false_for_effects_only() {
    let src = "contract Bar { effects io }";
    let file = assura_parser::parse_unwrap(src);
    assert!(!has_verifiable_clauses(&file));
}

#[test]
fn has_verifiable_clauses_false_for_empty() {
    let src = "contract Empty { }";
    let file = assura_parser::parse_unwrap(src);
    assert!(!has_verifiable_clauses(&file));
}

// =======================================================================
// T115: IncrementalCompiler tests
// =======================================================================

#[test]
fn incremental_dirty_on_register() {
    let mut ic = IncrementalCompiler::new();
    ic.register_module("main".into(), "abc".into());
    assert_eq!(ic.dirty_modules().len(), 1);
}

#[test]
fn incremental_clean_after_check() {
    let mut ic = IncrementalCompiler::new();
    ic.register_module("main".into(), "abc".into());
    ic.mark_checked("main", 100);
    assert!(ic.dirty_modules().is_empty());
}

#[test]
fn incremental_cascade_dirty() {
    let mut ic = IncrementalCompiler::new();
    ic.register_module("lib".into(), "aaa".into());
    ic.register_module("main".into(), "bbb".into());
    ic.add_dependency("main".into(), "lib".into());
    ic.mark_checked("lib", 1);
    ic.mark_checked("main", 1);
    ic.mark_changed("lib");
    let dirty = ic.dirty_modules();
    assert!(dirty.contains(&"lib"));
    assert!(dirty.contains(&"main"));
}

#[test]
fn incremental_module_count() {
    let mut ic = IncrementalCompiler::new();
    ic.register_module("a".into(), "h1".into());
    ic.register_module("b".into(), "h2".into());
    assert_eq!(ic.module_count(), 2);
}

#[test]
fn incremental_default() {
    let ic = IncrementalCompiler::default();
    assert_eq!(ic.module_count(), 0);
}
