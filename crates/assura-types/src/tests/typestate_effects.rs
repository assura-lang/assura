use super::*;
// T034: Typestate checker tests
// -----------------------------------------------------------------------

#[test]
fn typestate_valid_sequence_passes() {
    // Valid transition sequence: Init -> Open -> Close
    let states = vec!["Init".into(), "Open".into(), "Closed".into()];
    let transitions = vec![
        ("open".into(), "Init".into(), "Open".into()),
        ("close".into(), "Open".into(), "Closed".into()),
    ];
    let mut checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

    assert!(checker.transition("open", 5..9).is_ok());
    assert_eq!(checker.current_state(), "Open");
    assert!(checker.transition("close", 10..15).is_ok());
    assert_eq!(checker.current_state(), "Closed");
}

#[test]
fn typestate_wrong_state_a06001() {
    // Operation called in wrong state: close() requires Open, but
    // we are in Init.
    let states = vec!["Init".into(), "Open".into(), "Closed".into()];
    let transitions = vec![
        ("open".into(), "Init".into(), "Open".into()),
        ("close".into(), "Open".into(), "Closed".into()),
    ];
    let mut checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

    let err = checker.transition("close", 5..10).unwrap_err();
    assert_eq!(err.code, "A06001");
    assert!(err.message.contains("close"));
    assert!(err.message.contains("Init"));
    assert!(err.message.contains("Open"));
}

#[test]
fn typestate_not_linear_a06002() {
    // Typestate variables must be linear; this is checked separately.
    // The TypestateChecker itself produces A06002 when validate_linear
    // is called with is_linear=false.
    let states = vec!["Init".into(), "Open".into()];
    let transitions = vec![("open".into(), "Init".into(), "Open".into())];
    let checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

    let err = checker.validate_linear(false);
    assert!(err.is_some());
    let err = err.unwrap();
    assert_eq!(err.code, "A06002");
    assert!(err.message.contains("linear"));
}

#[test]
fn typestate_not_linear_ok_when_linear() {
    // When the variable IS linear, validate_linear returns None.
    let states = vec!["Init".into()];
    let checker = TypestateChecker::new(states, vec![], "Init".into(), 0..4);
    assert!(checker.validate_linear(true).is_none());
}

#[test]
fn typestate_undeclared_state_a06003() {
    // Operation transitions to a state not declared in `states:`.
    let states = vec!["Init".into(), "Open".into()];
    let transitions = vec![
        ("open".into(), "Init".into(), "Open".into()),
        // "Closed" is not in the declared states
        ("close".into(), "Open".into(), "Closed".into()),
    ];
    let checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

    let errors = checker.validate_transitions();
    assert!(!errors.is_empty());
    assert!(errors.iter().any(|e| e.code == "A06003"));
    assert!(errors.iter().any(|e| e.message.contains("Closed")));
}

#[test]
fn typestate_undeclared_source_state_a06003() {
    // Transition references a source state not in the declared states.
    let states = vec!["Init".into(), "Done".into()];
    let transitions = vec![
        // "Running" is not declared
        ("finish".into(), "Running".into(), "Done".into()),
    ];
    let checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

    let errors = checker.validate_transitions();
    assert!(!errors.is_empty());
    assert!(errors.iter().any(|e| e.code == "A06003"));
    assert!(errors.iter().any(|e| e.message.contains("Running")));
}

#[test]
fn typestate_ambiguous_after_branches_a06004() {
    // Diverging branches leave the object in different states.
    // After branch A: Open, after branch B: Closed => A06004.
    let states = vec!["Init".into(), "Open".into(), "Closed".into()];
    let transitions = vec![
        ("open".into(), "Init".into(), "Open".into()),
        ("close".into(), "Init".into(), "Closed".into()),
    ];

    let checker_a = {
        let mut c = TypestateChecker::new(states.clone(), transitions.clone(), "Init".into(), 0..4);
        c.transition("open", 5..9).unwrap();
        c
    };
    let checker_b = {
        let mut c = TypestateChecker::new(states, transitions, "Init".into(), 0..4);
        c.transition("close", 5..10).unwrap();
        c
    };

    let err = TypestateChecker::check_branch_consistency(&checker_a, &checker_b, 0..4);
    assert!(err.is_some());
    let err = err.unwrap();
    assert_eq!(err.code, "A06004");
    assert!(err.message.contains("Open"));
    assert!(err.message.contains("Closed"));
}

#[test]
fn typestate_consistent_branches_same_state_ok() {
    // Both branches leave the object in the same state: no error.
    let states = vec!["Init".into(), "Open".into()];
    let transitions = vec![("open".into(), "Init".into(), "Open".into())];

    let checker_a = {
        let mut c = TypestateChecker::new(states.clone(), transitions.clone(), "Init".into(), 0..4);
        c.transition("open", 5..9).unwrap();
        c
    };
    let checker_b = {
        let mut c = TypestateChecker::new(states, transitions, "Init".into(), 0..4);
        c.transition("open", 5..9).unwrap();
        c
    };

    let err = TypestateChecker::check_branch_consistency(&checker_a, &checker_b, 0..4);
    assert!(err.is_none());
}

#[test]
fn typestate_multiple_transitions_sequence() {
    // Longer transition chain: Init -> Connecting -> Connected -> Closed
    let states = vec![
        "Init".into(),
        "Connecting".into(),
        "Connected".into(),
        "Closed".into(),
    ];
    let transitions = vec![
        ("connect".into(), "Init".into(), "Connecting".into()),
        (
            "established".into(),
            "Connecting".into(),
            "Connected".into(),
        ),
        ("disconnect".into(), "Connected".into(), "Closed".into()),
    ];
    let mut checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

    assert!(checker.transition("connect", 5..12).is_ok());
    assert_eq!(checker.current_state(), "Connecting");
    assert!(checker.transition("established", 13..24).is_ok());
    assert_eq!(checker.current_state(), "Connected");
    assert!(checker.transition("disconnect", 25..35).is_ok());
    assert_eq!(checker.current_state(), "Closed");
}

#[test]
fn typestate_operation_not_found_a06001() {
    // Calling an operation that does not exist in any transition.
    let states = vec!["Init".into(), "Open".into()];
    let transitions = vec![("open".into(), "Init".into(), "Open".into())];
    let mut checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

    let err = checker.transition("nonexistent", 5..16).unwrap_err();
    assert_eq!(err.code, "A06001");
    assert!(err.message.contains("nonexistent"));
}

#[test]
fn typestate_valid_transitions_no_errors() {
    // All transitions reference declared states: no errors.
    let states = vec!["Init".into(), "Open".into(), "Closed".into()];
    let transitions = vec![
        ("open".into(), "Init".into(), "Open".into()),
        ("close".into(), "Open".into(), "Closed".into()),
    ];
    let checker = TypestateChecker::new(states, transitions, "Init".into(), 0..4);

    let errors = checker.validate_transitions();
    assert!(errors.is_empty());
}

#[test]
fn typestate_initial_state() {
    // Checker starts in the declared initial state.
    let states = vec!["Start".into(), "End".into()];
    let transitions = vec![("finish".into(), "Start".into(), "End".into())];
    let checker = TypestateChecker::new(states, transitions, "Start".into(), 0..5);

    assert_eq!(checker.current_state(), "Start");
}

// -----------------------------------------------------------------------
// T036-T037: Effect checker tests
// -----------------------------------------------------------------------

// -- EffectSet construction and display --

#[test]
fn effect_set_pure_is_empty() {
    let set = EffectSet::pure();
    assert!(set.is_pure());
    assert!(set.is_empty());
    assert_eq!(set.len(), 0);
    assert_eq!(format!("{set}"), "pure");
}

#[test]
fn effect_set_from_iter_basic() {
    let set = EffectSet::from_effect_names(["io", "mem"]);
    assert!(!set.is_pure());
    assert_eq!(set.len(), 2);
    assert!(set.contains("io"));
    assert!(set.contains("mem"));
    assert!(!set.contains("net"));
}

#[test]
fn effect_set_from_iter_pure_ignored() {
    // "pure" in the iterator should be ignored (it means empty set)
    let set = EffectSet::from_effect_names(["pure"]);
    assert!(set.is_pure());
    assert!(set.is_empty());
}

#[test]
fn effect_set_from_iter_pure_mixed() {
    // "pure" mixed with others: pure is dropped, others kept
    let set = EffectSet::from_effect_names(["pure", "io"]);
    assert!(!set.is_pure());
    assert_eq!(set.len(), 1);
    assert!(set.contains("io"));
}

#[test]
fn effect_set_insert() {
    let mut set = EffectSet::pure();
    set.insert("io".into());
    assert!(!set.is_pure());
    assert!(set.contains("io"));
}

#[test]
fn effect_set_insert_pure_noop() {
    let mut set = EffectSet::pure();
    set.insert("pure".into());
    assert!(set.is_pure());
}

#[test]
fn effect_set_display_sorted() {
    let set = EffectSet::from_effect_names(["mem", "io", "alloc"]);
    // Display should sort effects alphabetically
    assert_eq!(format!("{set}"), "{alloc, io, mem}");
}

// -- EffectChecker: known effects --

#[test]
fn effect_checker_knows_builtins() {
    let checker = EffectChecker::new();
    assert!(checker.is_known("io"));
    assert!(checker.is_known("mem"));
    assert!(checker.is_known("net"));
    assert!(checker.is_known("fs"));
    assert!(checker.is_known("rng"));
    assert!(checker.is_known("time"));
    assert!(checker.is_known("alloc"));
    assert!(checker.is_known("console.read"));
    assert!(checker.is_known("console.write"));
    assert!(checker.is_known("filesystem.read"));
    assert!(checker.is_known("filesystem.write"));
    assert!(checker.is_known("network.connect"));
    assert!(checker.is_known("network.send"));
    assert!(checker.is_known("network.receive"));
    assert!(checker.is_known("database"));
    assert!(checker.is_known("database.read"));
    assert!(checker.is_known("database.write"));
    assert!(checker.is_known("logging"));
    assert!(checker.is_known("log.debug"));
    assert!(checker.is_known("log.info"));
    assert!(checker.is_known("log.warn"));
    assert!(checker.is_known("log.error"));
    assert!(checker.is_known("time.read"));
    assert!(checker.is_known("random"));
    assert!(checker.is_known("diverge"));
}

#[test]
fn effect_checker_unknown_effect() {
    let checker = EffectChecker::new();
    assert!(!checker.is_known("teleport"));
    assert!(!checker.is_known("quantum"));
}

// -- A07003: unknown effect name --

#[test]
fn effect_check_known_all_valid() {
    let checker = EffectChecker::new();
    let set = EffectSet::from_effect_names(["io", "mem", "database"]);
    let errors = checker.check_known(&set, &(0..10));
    assert!(errors.is_empty());
}

#[test]
fn effect_check_known_unknown_a07003() {
    let checker = EffectChecker::new();
    let set = EffectSet::from_effect_names(["io", "teleport"]);
    let errors = checker.check_known(&set, &(0..10));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A07003");
    assert!(errors[0].message.contains("teleport"));
}

#[test]
fn effect_check_known_multiple_unknown_a07003() {
    let checker = EffectChecker::new();
    let set = EffectSet::from_effect_names(["teleport", "quantum"]);
    let errors = checker.check_known(&set, &(0..10));
    assert_eq!(errors.len(), 2);
    assert!(errors.iter().all(|e| e.code == "A07003"));
}

// -- Hierarchy expansion --

#[test]
fn effect_expand_io_includes_subeffects() {
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io"]);
    let expanded = checker.expand(&declared);
    assert!(expanded.contains("io"));
    assert!(expanded.contains("console.read"));
    assert!(expanded.contains("console.write"));
    assert!(expanded.contains("filesystem.read"));
    assert!(expanded.contains("filesystem.write"));
    assert!(expanded.contains("network.connect"));
    assert!(expanded.contains("network.send"));
    assert!(expanded.contains("network.receive"));
    assert!(expanded.contains("time.read"));
    assert!(expanded.contains("random"));
}

#[test]
fn effect_expand_database_includes_subeffects() {
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["database"]);
    let expanded = checker.expand(&declared);
    assert!(expanded.contains("database"));
    assert!(expanded.contains("database.read"));
    assert!(expanded.contains("database.write"));
}

#[test]
fn effect_expand_logging_includes_subeffects() {
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["logging"]);
    let expanded = checker.expand(&declared);
    assert!(expanded.contains("logging"));
    assert!(expanded.contains("log.debug"));
    assert!(expanded.contains("log.info"));
    assert!(expanded.contains("log.warn"));
    assert!(expanded.contains("log.error"));
}

#[test]
fn effect_expand_leaf_effect_no_change() {
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["console.read"]);
    let expanded = checker.expand(&declared);
    assert_eq!(expanded.len(), 1);
    assert!(expanded.contains("console.read"));
}

#[test]
fn effect_expand_pure_stays_empty() {
    let checker = EffectChecker::new();
    let declared = EffectSet::pure();
    let expanded = checker.expand(&declared);
    assert!(expanded.is_pure());
}

// -- Containment checks: positive (no errors) --

#[test]
fn effect_containment_pure_calling_pure_ok() {
    // Pure function calling another pure function: no errors
    let checker = EffectChecker::new();
    let declared = EffectSet::pure();
    let actual = EffectSet::pure();
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert!(errors.is_empty());
}

#[test]
fn effect_containment_declared_superset_ok() {
    // Declared {io, mem}, actual {mem}: mem is subset, OK
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io", "mem"]);
    let actual = EffectSet::from_effect_names(["mem"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert!(errors.is_empty());
}

#[test]
fn effect_containment_exact_match_ok() {
    // Declared and actual are identical: OK
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io", "mem"]);
    let actual = EffectSet::from_effect_names(["io", "mem"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert!(errors.is_empty());
}

#[test]
fn effect_containment_hierarchy_io_covers_console_ok() {
    // Declared {io}, actual {console.read}: io expands to include
    // console.read, so this is OK
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io"]);
    let actual = EffectSet::from_effect_names(["console.read"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert!(errors.is_empty());
}

#[test]
fn effect_containment_hierarchy_io_covers_network_ok() {
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io"]);
    let actual = EffectSet::from_effect_names(["network.send", "network.receive"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert!(errors.is_empty());
}

#[test]
fn effect_containment_hierarchy_database_covers_read_ok() {
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["database"]);
    let actual = EffectSet::from_effect_names(["database.read"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert!(errors.is_empty());
}

#[test]
fn effect_containment_hierarchy_logging_covers_all_levels_ok() {
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["logging"]);
    let actual = EffectSet::from_effect_names(["log.debug", "log.info", "log.warn", "log.error"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert!(errors.is_empty());
}

#[test]
fn effect_containment_declared_io_actual_empty_ok() {
    // Declared {io}, actual empty (pure body): always OK
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io"]);
    let actual = EffectSet::pure();
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert!(errors.is_empty());
}

// -- A07002: pure function performs effect --

#[test]
fn effect_containment_pure_performs_io_a07002() {
    // Pure function (empty declared set) performs io: A07002
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
fn effect_containment_pure_performs_mem_a07002() {
    let checker = EffectChecker::new();
    let declared = EffectSet::pure();
    let actual = EffectSet::from_effect_names(["mem"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A07002");
    assert!(errors[0].message.contains("mem"));
}

#[test]
fn effect_containment_pure_performs_multiple_a07002() {
    // Pure function performs multiple effects: one A07002 per effect
    let checker = EffectChecker::new();
    let declared = EffectSet::pure();
    let actual = EffectSet::from_effect_names(["io", "mem"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert_eq!(errors.len(), 2);
    assert!(errors.iter().all(|e| e.code == "A07002"));
}

// -- A07001: undeclared effect --

#[test]
fn effect_containment_undeclared_effect_a07001() {
    // Declared {io}, actual {io, mem}: mem is not declared => A07001
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io"]);
    let actual = EffectSet::from_effect_names(["io", "mem"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A07001");
    assert!(errors[0].message.contains("mem"));
}

#[test]
fn effect_containment_leaf_without_parent_a07001() {
    // Declared {console.read}, actual {console.write}: different leaf
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["console.read"]);
    let actual = EffectSet::from_effect_names(["console.write"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A07001");
    assert!(errors[0].message.contains("console.write"));
}

#[test]
fn effect_containment_database_without_io_a07001() {
    // Declared {io}, actual {database.read}: database is not under io
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io"]);
    let actual = EffectSet::from_effect_names(["database.read"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A07001");
    assert!(errors[0].message.contains("database.read"));
}

#[test]
fn effect_containment_multiple_undeclared_a07001() {
    // Declared {mem}, actual {io, database}: two undeclared effects
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["mem"]);
    let actual = EffectSet::from_effect_names(["io", "database"]);
    let errors = checker.check_containment(&declared, &actual, &(0..10));
    assert_eq!(errors.len(), 2);
    assert!(errors.iter().all(|e| e.code == "A07001"));
}

// -- Effect containment across call chain (T037 specific) --

#[test]
fn effect_containment_call_chain() {
    // Simulate: fn outer() effects {io} calls fn inner() effects {io, mem}
    // inner's actual effects must be subset of outer's declared.
    // mem is not in outer's declared set => A07001 for the call chain.
    let checker = EffectChecker::new();
    let outer_declared = EffectSet::from_effect_names(["io"]);
    // inner's effects propagate to outer's body
    let outer_actual = EffectSet::from_effect_names(["io", "mem"]);
    let errors = checker.check_containment(&outer_declared, &outer_actual, &(0..10));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A07001");
    assert!(errors[0].message.contains("mem"));
}

#[test]
fn effect_containment_call_chain_pure_callee_ok() {
    // fn outer() effects {io} calls fn inner() effects {pure}
    // pure is always a subset: OK
    let checker = EffectChecker::new();
    let outer_declared = EffectSet::from_effect_names(["io"]);
    let outer_actual = EffectSet::pure();
    let errors = checker.check_containment(&outer_declared, &outer_actual, &(0..10));
    assert!(errors.is_empty());
}

// -- Edge cases --

#[test]
fn effect_set_dedup() {
    // Duplicate effect names in iterator are deduplicated
    let set = EffectSet::from_effect_names(["io", "io", "mem", "mem"]);
    assert_eq!(set.len(), 2);
}

#[test]
fn effect_checker_default_trait() {
    // Default implementation works
    let checker = EffectChecker::default();
    assert!(checker.is_known("io"));
}

#[test]
fn effect_expand_multiple_groups() {
    // Expanding {io, database} should include sub-effects of both
    let checker = EffectChecker::new();
    let declared = EffectSet::from_effect_names(["io", "database"]);
    let expanded = checker.expand(&declared);
    assert!(expanded.contains("console.read"));
    assert!(expanded.contains("database.write"));
}

#[test]
fn effect_containment_span_preserved() {
    // Verify that the span from the input is preserved in errors
    let checker = EffectChecker::new();
    let declared = EffectSet::pure();
    let actual = EffectSet::from_effect_names(["io"]);
    let errors = checker.check_containment(&declared, &actual, &(42..99));
    assert_eq!(errors[0].span, 42..99);
}

#[test]
fn effect_set_iter() {
    let set = EffectSet::from_effect_names(["io", "mem"]);
    let mut items: Vec<&str> = set.iter().collect();
    items.sort();
    assert_eq!(items, vec!["io", "mem"]);
}

// -----------------------------------------------------------------------
