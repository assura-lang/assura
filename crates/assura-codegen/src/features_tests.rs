use super::*;
use assura_ast::{Expr, Literal, SpExpr, Spanned};

fn mk_clause(kind: ClauseKind, body: SpExpr) -> Clause {
    Clause {
        kind,
        body,
        effect_variables: vec![],
    }
}

fn mk_other(kind: &str) -> Clause {
    mk_clause(
        ClauseKind::Other(kind.into()),
        Spanned::no_span(Expr::Literal(Literal::Bool(true))),
    )
}

fn mk_other_ident(kind: &str, ident: &str) -> Clause {
    mk_clause(
        ClauseKind::Other(kind.into()),
        Spanned::no_span(Expr::Ident(ident.into())),
    )
}

// ---- CORE features ----

#[test]
fn axiomatic_definition() {
    let clause = mk_other("axiom");
    let code = generate_axiomatic_definition(&clause);
    assert!(code.contains("Axiomatic definition"));
    assert!(code.contains("debug_assert!(true"));
}

#[test]
fn ghost_compile_check() {
    let code = generate_ghost_compile_check("my_ghost");
    assert!(code.contains("ghost compile-time"));
    assert!(code.contains("my_ghost"));
    assert!(code.contains("cfg(not(debug_assertions))"));
}

#[test]
fn opaque_function() {
    let code = generate_opaque_function("secret_fn");
    assert!(code.contains("opaque"));
    assert!(code.contains("secret_fn"));
}

#[test]
fn liveness_check() {
    let clause = mk_other_ident("liveness", "progress");
    let code = generate_liveness_check(&clause);
    assert!(code.contains("liveness"));
    assert!(code.contains("debug_assert!(progress"));
}

// ---- MEM features ----

#[test]
fn region_annotation() {
    let clause = mk_other_ident("region", "heap");
    let code = generate_region_annotation(&clause);
    assert!(code.contains("region constraint"));
    assert!(code.contains("debug_assert!"));
}

#[test]
fn allocator_check() {
    let clause = mk_other("allocator");
    let code = generate_allocator_check(&clause);
    assert!(code.contains("allocator invariant"));
}

#[test]
fn circular_buffer_check() {
    let clause = mk_other("circular_buffer");
    let code = generate_circular_buffer_check(&clause);
    assert!(code.contains("circular buffer invariant"));
}

// ---- TYPE features ----

#[test]
fn structural_invariant() {
    let clause = mk_other_ident("structural_invariant", "sorted");
    let code = generate_structural_invariant(&clause);
    assert!(code.contains("structural_invariant"));
    assert!(code.contains("debug_assert!(sorted"));
}

#[test]
fn error_propagation_check() {
    let clause = mk_other("must_propagate");
    let code = generate_error_propagation_check(&clause);
    assert!(code.contains("error_propagation"));
}

// ---- SEC features ----

#[test]
fn constant_time_annotation() {
    let code = generate_constant_time_annotation("compare_digest");
    assert!(code.contains("constant_time"));
    assert!(code.contains("compare_digest"));
}

#[test]
fn crypto_conformance() {
    let clause = mk_other_ident("conforms", "AES256");
    let code = generate_crypto_conformance_check(&clause);
    assert!(code.contains("crypto conformance"));
    assert!(code.contains("AES256"));
}

// ---- CONC features ----

#[test]
fn callback_reentrancy_guard() {
    let code = generate_callback_reentrancy_guard("on_event");
    assert!(code.contains("callback reentrancy guard"));
    assert!(code.contains("ON_EVENT"));
    assert!(code.contains("thread_local!"));
}

#[test]
fn deterministic_annotation() {
    let code = generate_deterministic_annotation("hash_fn");
    assert!(code.contains("deterministic"));
    assert!(code.contains("hash_fn"));
}

#[test]
fn lock_order_annotation() {
    let clause = mk_other_ident("lock_order", "mutex_a");
    let code = generate_lock_order_annotation(&clause);
    assert!(code.contains("lock_order"));
}

#[test]
fn deadline_check() {
    let clause = mk_other_ident("deadline", "timeout_ms");
    let code = generate_deadline_check(&clause);
    assert!(code.contains("deadline"));
}

// ---- STOR features ----

#[test]
fn crash_recovery() {
    let clause = mk_other("crash_recovery");
    let code = generate_crash_recovery_check(&clause);
    assert!(code.contains("crash_recovery"));
}

#[test]
fn page_cache() {
    let clause = mk_other("page_cache");
    let code = generate_page_cache_check(&clause);
    assert!(code.contains("page_cache"));
}

#[test]
fn mvcc_check() {
    let clause = mk_other("mvcc");
    let code = generate_mvcc_check(&clause);
    assert!(code.contains("mvcc snapshot isolation"));
}

#[test]
fn rollback_check() {
    let clause = mk_other("rollback");
    let code = generate_rollback_check(&clause);
    assert!(code.contains("rollback savepoint"));
}

#[test]
fn monotonic_check() {
    let clause = mk_other_ident("monotonic", "counter");
    let code = generate_monotonic_check(&clause);
    assert!(code.contains("monotonic state"));
}

#[test]
fn storage_failure() {
    let clause = mk_other("storage_failure");
    let code = generate_storage_failure_check(&clause);
    assert!(code.contains("storage_failure"));
}

// ---- FMT features ----

#[test]
fn binary_format() {
    let clause = mk_other("binary_format");
    let code = generate_binary_format_check(&clause);
    assert!(code.contains("binary_format"));
}

#[test]
fn bit_level() {
    let clause = mk_other("bit_level");
    let code = generate_bit_level_check(&clause);
    assert!(code.contains("bit_level"));
}

#[test]
fn string_encoding() {
    let clause = mk_other("string_encoding");
    let code = generate_string_encoding_check(&clause);
    assert!(code.contains("string_encoding"));
}

#[test]
fn checksum() {
    let clause = mk_other("checksum");
    let code = generate_checksum_check(&clause);
    assert!(code.contains("checksum integrity"));
}

#[test]
fn protocol_grammar() {
    let clause = mk_other("protocol_grammar");
    let code = generate_protocol_grammar_check(&clause);
    assert!(code.contains("protocol_grammar"));
}

// ---- NUM features ----

#[test]
fn numerical_precision() {
    let clause = mk_other("precision");
    let code = generate_numerical_precision_check(&clause);
    assert!(code.contains("numerical_precision"));
}

#[test]
fn precomputed_table() {
    let clause = mk_other("precomputed_table");
    let code = generate_precomputed_table_check(&clause);
    assert!(code.contains("precomputed_table"));
}

// ---- PLAT features ----

#[test]
fn platform_abstraction() {
    let clause = mk_other("platform");
    let code = generate_platform_abstraction(&clause);
    assert!(code.contains("platform_abstraction"));
}

#[test]
fn feature_flag() {
    let clause = mk_other("feature_flag");
    let code = generate_feature_flag(&clause);
    assert!(code.contains("feature_flag"));
}

#[test]
fn resource_limit() {
    let clause = mk_other("resource_limit");
    let code = generate_resource_limit_check(&clause);
    assert!(code.contains("resource_limit"));
}

// ---- PERF features ----

#[test]
fn unsafe_escape() {
    let clause = mk_other("unsafe_escape");
    let code = generate_unsafe_escape(&clause);
    assert!(code.contains("unsafe_escape"));
}

#[test]
fn complexity_bound() {
    let clause = mk_other("complexity");
    let code = generate_complexity_bound(&clause);
    assert!(code.contains("complexity_bound"));
}

// ---- TEST features ----

#[test]
fn behavioral_equiv() {
    let clause = mk_other_ident("behavioral_equiv", "reference_impl");
    let code = generate_behavioral_equiv_test("my_fn", &clause);
    assert!(code.contains("behavioral_equiv"));
    assert!(code.contains("my_fn"));
}

#[test]
fn multi_pass_refinement() {
    let clause = mk_other("multi_pass");
    let code = generate_multi_pass_refinement(&clause);
    assert!(code.contains("multi_pass_refinement"));
}

// ---- MISC features ----

#[test]
fn incremental_contract() {
    let clause = mk_other("incremental");
    let code = generate_incremental_contract(&clause);
    assert!(code.contains("incremental_contract"));
}

#[test]
fn scoped_invariant() {
    let clause = mk_other("scoped_invariant");
    let code = generate_scoped_invariant(&clause);
    assert!(code.contains("scoped_invariant"));
}

// ---- Compile-time enforcement ----

#[test]
fn compile_time_ghost_erasure_fn() {
    let code = compile_time_ghost_erasure("g");
    assert!(code.contains("compile_time_ghost"));
}

#[test]
fn compile_time_taint_fn() {
    let code = compile_time_taint("x");
    assert!(code.contains("compile_time_taint"));
}

#[test]
fn compile_time_constant_time_fn() {
    let code = compile_time_constant_time("ct");
    assert!(code.contains("compile_time_constant_time"));
}

#[test]
fn compile_time_zeroize_fn() {
    let code = compile_time_zeroize("key");
    assert!(code.contains("compile_time_zeroize"));
}

#[test]
fn compile_time_shared_memory_fn() {
    let code = compile_time_shared_memory("buf");
    assert!(code.contains("compile_time_shared_memory"));
}

#[test]
fn compile_time_weak_memory_fn() {
    let code = compile_time_weak_memory();
    assert!(code.contains("compile_time_ordering"));
}

#[test]
fn compile_time_fixed_width_fn() {
    let code = compile_time_fixed_width();
    assert!(code.contains("compile_time_fixed_width"));
}

#[test]
fn compile_time_interface_fn() {
    let code = compile_time_interface("Trait");
    assert!(code.contains("compile_time_interface"));
}

#[test]
fn compile_time_error_propagation_fn() {
    let code = compile_time_error_propagation();
    assert!(code.contains("compile_time_error_propagation"));
}

#[test]
fn compile_time_feature_flag_fn() {
    let code = compile_time_feature_flag("opt");
    assert!(code.contains("compile_time_feature_flag"));
}

#[test]
fn compile_time_unsafe_escape_fn() {
    let code = compile_time_unsafe_escape("raw");
    assert!(code.contains("compile_time_unsafe_escape"));
}

#[test]
fn compile_time_numerical_precision_fn() {
    let code = compile_time_numerical_precision();
    assert!(code.contains("compile_time_numerical_precision"));
}

#[test]
fn compile_time_resource_limit_fn() {
    let code = compile_time_resource_limit();
    assert!(code.contains("compile_time_resource_limit"));
}

#[test]
fn compile_time_binary_format_fn() {
    let code = compile_time_binary_format();
    assert!(code.contains("compile_time_binary_format"));
}

#[test]
fn compile_time_monotonic_fn() {
    let code = compile_time_monotonic();
    assert!(code.contains("compile_time_monotonic"));
}

// ---- generate_feature_clause dispatch ----

#[test]
fn dispatch_ghost() {
    let clause = mk_other("ghost");
    let mut code = String::new();
    assert!(generate_feature_clause(&clause, "fn1", &mut code));
    assert!(code.contains("ghost compile-time"));
    assert!(code.contains("compile_time_ghost"));
}

#[test]
fn dispatch_axiom() {
    let clause = mk_other("axiom");
    let mut code = String::new();
    assert!(generate_feature_clause(&clause, "fn1", &mut code));
    assert!(code.contains("Axiomatic definition"));
}

#[test]
fn dispatch_axiomatic_synonym() {
    let clause = mk_other("axiomatic");
    let mut code = String::new();
    assert!(generate_feature_clause(&clause, "fn1", &mut code));
    assert!(code.contains("Axiomatic definition"));
}

#[test]
fn dispatch_opaque() {
    let clause = mk_other("opaque");
    let mut code = String::new();
    assert!(generate_feature_clause(&clause, "fn1", &mut code));
    assert!(code.contains("opaque"));
}

#[test]
fn dispatch_liveness() {
    let clause = mk_other("liveness");
    let mut code = String::new();
    assert!(generate_feature_clause(&clause, "fn1", &mut code));
    assert!(code.contains("liveness"));
}

#[test]
fn dispatch_eventually_synonym() {
    let clause = mk_other("eventually");
    let mut code = String::new();
    assert!(generate_feature_clause(&clause, "fn1", &mut code));
    assert!(code.contains("liveness"));
}

#[test]
fn dispatch_region() {
    let clause = mk_other("region");
    let mut code = String::new();
    assert!(generate_feature_clause(&clause, "fn1", &mut code));
    assert!(code.contains("region constraint"));
}

#[test]
fn dispatch_taint() {
    let clause = mk_other("taint");
    let mut code = String::new();
    assert!(generate_feature_clause(&clause, "fn1", &mut code));
    assert!(code.contains("compile_time_taint"));
}

#[test]
fn dispatch_constant_time() {
    let clause = mk_other("constant_time");
    let mut code = String::new();
    assert!(generate_feature_clause(&clause, "fn1", &mut code));
    assert!(code.contains("constant_time"));
}

#[test]
fn dispatch_zeroize() {
    let clause = mk_other("zeroize");
    let mut code = String::new();
    assert!(generate_feature_clause(&clause, "fn1", &mut code));
    assert!(code.contains("compile_time_zeroize"));
}

#[test]
fn dispatch_shared_memory() {
    let clause = mk_other("shared_memory");
    let mut code = String::new();
    assert!(generate_feature_clause(&clause, "fn1", &mut code));
    assert!(code.contains("compile_time_shared_memory"));
}

#[test]
fn dispatch_callback() {
    let clause = mk_other("callback");
    let mut code = String::new();
    assert!(generate_feature_clause(&clause, "fn1", &mut code));
    assert!(code.contains("callback reentrancy guard"));
}

#[test]
fn dispatch_deterministic() {
    let clause = mk_other("deterministic");
    let mut code = String::new();
    assert!(generate_feature_clause(&clause, "fn1", &mut code));
    assert!(code.contains("deterministic"));
}

#[test]
fn dispatch_crash_recovery() {
    let clause = mk_other("crash_recovery");
    let mut code = String::new();
    assert!(generate_feature_clause(&clause, "fn1", &mut code));
    assert!(code.contains("crash_recovery"));
}

#[test]
fn dispatch_monotonic() {
    let clause = mk_other("monotonic");
    let mut code = String::new();
    assert!(generate_feature_clause(&clause, "fn1", &mut code));
    assert!(code.contains("monotonic"));
    assert!(code.contains("compile_time_monotonic"));
}

#[test]
fn dispatch_binary_format() {
    let clause = mk_other("binary_format");
    let mut code = String::new();
    assert!(generate_feature_clause(&clause, "fn1", &mut code));
    assert!(code.contains("binary_format"));
    assert!(code.contains("compile_time_binary_format"));
}

#[test]
fn dispatch_feature_flag() {
    let clause = mk_other("feature_flag");
    let mut code = String::new();
    assert!(generate_feature_clause(&clause, "fn1", &mut code));
    assert!(code.contains("feature_flag"));
    assert!(code.contains("compile_time_feature_flag"));
}

#[test]
fn dispatch_unsafe_escape() {
    let clause = mk_other("unsafe_escape");
    let mut code = String::new();
    assert!(generate_feature_clause(&clause, "fn1", &mut code));
    assert!(code.contains("unsafe_escape"));
    assert!(code.contains("compile_time_unsafe_escape"));
}

#[test]
fn dispatch_precision() {
    let clause = mk_other("precision");
    let mut code = String::new();
    assert!(generate_feature_clause(&clause, "fn1", &mut code));
    assert!(code.contains("numerical_precision"));
    assert!(code.contains("compile_time_numerical_precision"));
}

#[test]
fn dispatch_resource_limit() {
    let clause = mk_other("resource_limit");
    let mut code = String::new();
    assert!(generate_feature_clause(&clause, "fn1", &mut code));
    assert!(code.contains("resource_limit"));
    assert!(code.contains("compile_time_resource_limit"));
}

#[test]
fn dispatch_unknown_returns_false() {
    let clause = mk_other("not_a_known_feature");
    let mut code = String::new();
    assert!(!generate_feature_clause(&clause, "fn1", &mut code));
    assert!(code.is_empty());
}

#[test]
fn dispatch_non_other_clause_returns_false() {
    let clause = mk_clause(
        ClauseKind::Requires,
        Spanned::no_span(Expr::Literal(Literal::Bool(true))),
    );
    let mut code = String::new();
    assert!(!generate_feature_clause(&clause, "fn1", &mut code));
    assert!(code.is_empty());
}

// ---- generate_all_feature_clauses ----

#[test]
fn all_features_dispatches_multiple() {
    let clauses = vec![
        mk_other("ghost"),
        mk_other("region"),
        mk_clause(
            ClauseKind::Requires,
            Spanned::no_span(Expr::Literal(Literal::Bool(true))),
        ),
    ];
    let mut code = String::new();
    generate_all_feature_clauses(&clauses, "fn1", &mut code);
    assert!(code.contains("ghost compile-time"));
    assert!(code.contains("region constraint"));
    // Requires clause is not a feature clause, should not add anything
}

#[test]
fn all_features_empty_clauses() {
    let mut code = String::new();
    generate_all_feature_clauses(&[], "fn1", &mut code);
    assert!(code.is_empty());
}

// ---- #187: Real enforcement tests ----

#[test]
fn frame_conditions_generates_debug_assert() {
    let clause = mk_other_ident("frame", "table");
    let code = compile_time_frame(&clause, "update_fn");
    assert!(
        code.contains("debug_assert_eq!"),
        "frame conditions should generate debug_assert_eq!, got: {code}"
    );
    assert!(
        code.contains("compile_time_frame"),
        "should contain feature identifier"
    );
}

#[test]
fn frame_conditions_empty_emits_compile_error() {
    let clause = mk_clause(
        ClauseKind::Other("frame".into()),
        Spanned::no_span(Expr::Raw(vec![])),
    );
    let code = compile_time_frame(&clause, "bad_fn");
    assert!(
        code.contains("compile_error!"),
        "empty frame should generate compile_error!, got: {code}"
    );
}

#[test]
fn trigger_pattern_validates_non_empty() {
    let clause = mk_other_ident("trigger_pattern", "f(x)");
    let code = compile_time_trigger_pattern(&clause);
    assert!(
        code.contains("compile_time_trigger_pattern"),
        "should contain feature identifier, got: {code}"
    );
    assert!(
        !code.contains("compile_error!"),
        "non-empty trigger should not produce compile_error!, got: {code}"
    );
}

#[test]
fn trigger_pattern_empty_emits_compile_error() {
    let clause = mk_clause(
        ClauseKind::Other("trigger_pattern".into()),
        Spanned::no_span(Expr::Raw(vec![])),
    );
    let code = compile_time_trigger_pattern(&clause);
    assert!(
        code.contains("compile_error!"),
        "empty trigger should generate compile_error!, got: {code}"
    );
}

#[test]
fn dependent_types_generates_newtype() {
    let clause = mk_other_ident("dependent", "secret");
    let code = compile_time_dependent_types(&clause);
    assert!(
        code.contains("struct Label_secret"),
        "should generate newtype wrapper, got: {code}"
    );
    assert!(
        code.contains("into_inner"),
        "should generate accessor method, got: {code}"
    );
}

#[test]
fn dependent_types_empty_emits_compile_error() {
    let clause = mk_clause(
        ClauseKind::Other("dependent".into()),
        Spanned::no_span(Expr::Raw(vec![])),
    );
    let code = compile_time_dependent_types(&clause);
    assert!(
        code.contains("compile_error!"),
        "empty label should generate compile_error!, got: {code}"
    );
}

#[test]
fn frame_conditions_multi_field() {
    // Real-world pattern: modifies { ctx.peer_point, ctx.shared_secret }
    let clause = mk_clause(
        ClauseKind::Other("frame".into()),
        Spanned::no_span(Expr::Raw(vec![
            "ctx.peer_point".into(),
            ",".into(),
            "ctx.shared_secret".into(),
        ])),
    );
    let code = compile_time_frame(&clause, "ecdh_parse");
    assert!(
        code.contains("debug_assert_eq!(ctx.peer_point"),
        "should generate assert for first field, got: {code}"
    );
    assert!(
        code.contains("debug_assert_eq!(ctx.shared_secret"),
        "should generate assert for second field, got: {code}"
    );
}
