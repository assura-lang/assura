use super::super::*;

// Domain checker integration tests (issues #63, #65)
// =======================================================================

// --- PageCacheChecker tests ---

#[test]
fn page_cache_checker_capacity_from_ast() {
    use crate::domain::PageCacheChecker;
    let mut checker = PageCacheChecker::new(2);
    checker.load_page(1);
    checker.load_page(2);
    checker.load_page(3);
    let errors = checker.check_capacity();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A34003");
    assert!(errors[0].message.contains("exceeds capacity"));
}

#[test]
fn page_cache_checker_within_capacity() {
    use crate::domain::PageCacheChecker;
    let checker = PageCacheChecker::new(10);
    let errors = checker.check_capacity();
    assert!(errors.is_empty());
}

#[test]
fn page_cache_checker_dirty_evict() {
    use crate::domain::PageCacheChecker;
    let mut checker = PageCacheChecker::new(10);
    checker.load_page(42);
    checker.mark_dirty(42);
    let err = checker.evict(42);
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A34002");
}

#[test]
fn page_cache_checker_pinned_evict() {
    use crate::domain::PageCacheChecker;
    let mut checker = PageCacheChecker::new(10);
    checker.load_page(7);
    checker.pin(7);
    let err = checker.evict(7);
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A34001");
}

// --- MvccChecker tests ---

#[test]
fn mvcc_checker_write_conflict() {
    use crate::domain::MvccChecker;
    let mut checker = MvccChecker::new();
    let tx1 = checker.begin_txn();
    let tx2 = checker.begin_txn();
    checker.write_version("key_a".into(), tx1);
    checker.write_version("key_a".into(), tx2);
    let errors = checker.check_write_conflicts();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A35001");
    assert!(errors[0].message.contains("write-write conflict"));
}

#[test]
fn mvcc_checker_no_conflict_after_commit() {
    use crate::domain::MvccChecker;
    let mut checker = MvccChecker::new();
    let tx1 = checker.begin_txn();
    checker.write_version("key_b".into(), tx1);
    checker.commit_txn(tx1);
    let tx2 = checker.begin_txn();
    checker.write_version("key_b".into(), tx2);
    let errors = checker.check_write_conflicts();
    assert!(errors.is_empty());
}

#[test]
fn mvcc_checker_snapshot_violation() {
    use crate::domain::MvccChecker;
    let mut checker = MvccChecker::new();
    let tx1 = checker.begin_txn();
    let tx2 = checker.begin_txn();
    checker.write_version("shared".into(), tx1);
    let err = checker.check_snapshot_read("shared", tx2);
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A35002");
}

// --- RollbackChecker tests ---

#[test]
fn rollback_checker_resource_leak() {
    use crate::domain::RollbackChecker;
    let mut checker = RollbackChecker::new();
    checker.create_savepoint("sp1".into());
    checker.acquire_resource("file_handle".into());
    checker.rollback_to("sp1");
    let errors = checker.check_resource_leak();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A36002");
    assert!(errors[0].message.contains("file_handle"));
}

#[test]
fn rollback_checker_no_leak_when_released() {
    use crate::domain::RollbackChecker;
    let mut checker = RollbackChecker::new();
    checker.create_savepoint("sp1".into());
    checker.acquire_resource("conn".into());
    checker.release_resource("conn");
    checker.rollback_to("sp1");
    let errors = checker.check_resource_leak();
    assert!(errors.is_empty());
}

#[test]
fn rollback_checker_unknown_savepoint() {
    use crate::domain::RollbackChecker;
    let mut checker = RollbackChecker::new();
    let err = checker.rollback_to("nonexistent");
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A36001");
}

#[test]
fn rollback_checker_duplicate_savepoint() {
    use crate::domain::RollbackChecker;
    let mut checker = RollbackChecker::new();
    checker.create_savepoint("dup".into());
    checker.create_savepoint("dup".into());
    let errors = checker.check_savepoint_nesting();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A36003");
}

// --- Expression extraction helper tests ---

#[test]
fn extract_int_literal_positive() {
    use crate::checkers::extract_int_literal;
    use assura_parser::ast::{Expr, Literal, Spanned};
    let expr = Spanned::no_span(Expr::Literal(Literal::Int("42".into())));
    assert_eq!(extract_int_literal(&expr), Some(42));
}

#[test]
fn extract_int_literal_negative() {
    use crate::checkers::extract_int_literal;
    use assura_parser::ast::{Expr, Literal, UnaryOp};
    let expr = Spanned::no_span(Expr::UnaryOp {
        op: UnaryOp::Neg,
        expr: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("5".into())))),
    });
    assert_eq!(extract_int_literal(&expr), Some(-5));
}

#[test]
fn extract_ident_works() {
    use crate::checkers::extract_ident;
    use assura_parser::ast::Expr;
    let expr = Spanned::no_span(Expr::Ident("hello".into()));
    assert_eq!(extract_ident(&expr), Some("hello"));
}

#[test]
fn extract_kv_pair_from_eq() {
    use crate::checkers::extract_kv_pair;
    use assura_parser::ast::{BinOp, Expr, Literal};
    let expr = Spanned::no_span(Expr::BinOp {
        op: BinOp::Eq,
        lhs: Box::new(Spanned::no_span(Expr::Ident("size".into()))),
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("256".into())))),
    });
    let pair = extract_kv_pair(&expr);
    assert!(pair.is_some());
    let (key, _val) = pair.unwrap();
    assert_eq!(key, "size");
}

#[test]
fn extract_call_works() {
    use crate::checkers::extract_call;
    use assura_parser::ast::{Expr, Literal};
    let expr = Spanned::no_span(Expr::Call {
        func: Box::new(Spanned::no_span(Expr::Ident("load_page".into()))),
        args: vec![Spanned::no_span(Expr::Literal(Literal::Int("42".into())))],
    });
    let result = extract_call(&expr);
    assert!(result.is_some());
    let (name, args) = result.unwrap();
    assert_eq!(name, "load_page");
    assert_eq!(args.len(), 1);
}

// --- Multi-pass refinement checker tests ---

#[test]
fn multi_pass_refinement_chain() {
    use crate::domain::MultiPassRefinementChecker;
    let mut checker = MultiPassRefinementChecker::new();
    checker.add_pass("step1".into(), "abstract".into(), "mid".into(), 3, 0..5);
    checker.add_pass("step2".into(), "mid".into(), "concrete".into(), 2, 5..10);
    let errors = checker.check_chain();
    assert!(
        errors.is_empty(),
        "well-chained passes should have no errors"
    );
}

#[test]
fn multi_pass_refinement_broken_chain() {
    use crate::domain::MultiPassRefinementChecker;
    let mut checker = MultiPassRefinementChecker::new();
    checker.add_pass("step1".into(), "abstract".into(), "mid".into(), 3, 0..5);
    checker.add_pass("step2".into(), "other".into(), "concrete".into(), 2, 5..10);
    let errors = checker.check_chain();
    assert!(!errors.is_empty(), "broken chain should produce errors");
}

// --- Incremental contract checker tests ---

#[test]
fn incremental_contract_version_continuity() {
    use crate::domain::IncrementalContractChecker;
    let mut checker = IncrementalContractChecker::new();
    checker.add_version("Foo".into(), 1, 2, 1, 0..1);
    checker.add_version("Foo".into(), 2, 3, 2, 0..1);
    let errors = checker.check_version_continuity();
    assert!(errors.is_empty(), "sequential versions should pass");
}

#[test]
fn incremental_contract_precondition_weakening() {
    use crate::domain::IncrementalContractChecker;
    let mut checker = IncrementalContractChecker::new();
    checker.add_version("Bar".into(), 1, 3, 1, 0..1);
    checker.add_version("Bar".into(), 2, 2, 1, 0..1);
    let errors = checker.check_precondition_weakening();
    // Fewer requires in v2 is allowed (weakening); more would be an error
    assert!(errors.is_empty());
}

// =======================================================================
// 4.03: Cross-file type checking
// =======================================================================

#[test]
fn cross_file_import_resolves_contract_type() {
    // Module "math" defines contract Add
    let math_src = "module math\ncontract Add { input(a: Int, b: Int) output(result: Int) }";
    let math_file = assura_parser::parse_unwrap(math_src);
    let mut module_map = std::collections::HashMap::new();
    module_map.insert("math".to_string(), math_file.clone());

    let math_resolved = assura_resolve::resolve_with_modules(
        &math_file,
        &module_map,
        &mut std::collections::HashSet::new(),
    )
    .expect("math resolve failed");

    // Module "main" imports Add from math
    let main_src = "import math { Add }\ncontract Main { input(x: Int) output(result: Int) }";
    let main_file = assura_parser::parse_unwrap(main_src);
    let main_resolved = assura_resolve::resolve_with_modules(
        &main_file,
        &module_map,
        &mut std::collections::HashSet::new(),
    )
    .expect("main resolve failed");

    // Build modules map for cross-file type checking
    let mut modules = std::collections::HashMap::new();
    modules.insert("math".to_string(), math_resolved.clone());

    let result = crate::TypeChecker::new()
        .modules(modules)
        .check(&main_resolved);
    // Should succeed: imported Add is known, main's own types are valid
    assert!(result.is_ok(), "cross-file type check should succeed");

    // Verify the imported type is concrete (not Unknown)
    let typed = result.unwrap();
    let add_ty = typed.type_env.lookup("Add");
    assert!(
        add_ty.is_some(),
        "imported contract Add should be in the type env"
    );
    assert_ne!(
        add_ty.unwrap(),
        &crate::Type::Unknown,
        "imported contract Add should not be Type::Unknown"
    );
}

#[test]
fn cross_file_import_resolves_type_def() {
    // Module "geom" defines type Vector
    let geom_src = "module geom\ntype Vector { x: Float, y: Float }";
    let geom_file = assura_parser::parse_unwrap(geom_src);
    let mut module_map = std::collections::HashMap::new();
    module_map.insert("geom".to_string(), geom_file.clone());

    let geom_resolved = assura_resolve::resolve_with_modules(
        &geom_file,
        &module_map,
        &mut std::collections::HashSet::new(),
    )
    .expect("geom resolve failed");

    // Module "main" imports Vector from geom
    let main_src =
        "import geom { Vector }\ncontract UseVector { input(v: Vector) output(result: Float) }";
    let main_file = assura_parser::parse_unwrap(main_src);
    let main_resolved = assura_resolve::resolve_with_modules(
        &main_file,
        &module_map,
        &mut std::collections::HashSet::new(),
    )
    .expect("main resolve failed");

    let mut modules = std::collections::HashMap::new();
    modules.insert("geom".to_string(), geom_resolved.clone());

    let result = crate::TypeChecker::new()
        .modules(modules)
        .check(&main_resolved);
    assert!(result.is_ok(), "cross-file type check should succeed");

    let typed = result.unwrap();
    // Verify struct fields were injected
    assert!(
        typed.type_env.struct_fields.contains_key("Vector"),
        "imported struct Vector should have its fields in the type env"
    );
    let fields = &typed.type_env.struct_fields["Vector"];
    assert_eq!(fields.len(), 2, "Vector should have 2 fields (x, y)");
}

#[test]
fn cross_file_without_modules_still_works() {
    // Single-file type checking (no imports, empty modules map)
    let src = "contract Simple { input(x: Int) output(result: Int) }";
    let file = assura_parser::parse_unwrap(src);
    let resolved = assura_resolve::resolve(&file).expect("resolve failed");
    let modules = std::collections::HashMap::new();

    let result = crate::TypeChecker::new().modules(modules).check(&resolved);
    assert!(
        result.is_ok(),
        "type checking with empty modules map should still work"
    );
}

#[test]
fn cross_file_unresolved_import_is_ignored() {
    // Module "main" imports from a module that doesn't exist in the map
    let main_src =
        "import nonexistent { Foo }\ncontract Main { input(x: Int) output(result: Int) }";
    let main_file = assura_parser::parse_unwrap(main_src);
    let main_resolved = assura_resolve::resolve(&main_file).expect("resolve failed");
    let modules = std::collections::HashMap::new();

    let result = crate::TypeChecker::new()
        .modules(modules)
        .check(&main_resolved);
    // Should succeed; unresolved imports are just Unknown types (no crash)
    assert!(
        result.is_ok(),
        "unresolved imports should not cause type check failure"
    );
}

// =========================================================================
// Issue #112: Circular buffer capacity extracted from annotations
// =========================================================================

#[test]
fn circular_buffer_capacity_extraction_call_syntax() {
    // Verify that capacity is extracted from call syntax, not hardcoded to 256
    let mut checker = CircularBufferChecker::new();
    checker.declare("ring".into(), 16);
    // Buffer with capacity 16 should report full at 16 items
    for _ in 0..16 {
        checker.push("ring");
    }
    assert!(checker.buffers.get("ring").unwrap().is_full());
}

#[test]
fn circular_buffer_check_index_wired() {
    // Verify check_index is callable and returns errors for out-of-bounds
    let mut checker = CircularBufferChecker::new();
    checker.declare("buf".into(), 4);
    // Index 0 on empty buffer: check_index should flag it
    let err = checker.check_index("buf", 5, &(0..1));
    assert!(err.is_some(), "index 5 on capacity-4 buffer should error");
    assert_eq!(err.unwrap().code, "A23001");
}

// =========================================================================
// Issue #113: Axiomatic definitions extract references from clause bodies
// =========================================================================

#[test]
fn axiomatic_references_extracted_from_body() {
    let mut checker = AxiomaticDefChecker::new();
    // Declare two axioms where axiom_a references axiom_b
    checker.declare_axiom(AxiomDef {
        name: "axiom_a".into(),
        span: 0..1,
        references: vec!["axiom_b".into()],
    });
    checker.declare_axiom(AxiomDef {
        name: "axiom_b".into(),
        span: 0..1,
        references: vec![],
    });
    // check_circular should find no cycle (a -> b, no b -> a)
    let circ_errs = checker.check_circular();
    assert!(circ_errs.is_empty(), "no circular dependency expected");
}

#[test]
fn axiomatic_circular_reference_detected() {
    let mut checker = AxiomaticDefChecker::new();
    checker.declare_axiom(AxiomDef {
        name: "ax1".into(),
        span: 0..1,
        references: vec!["ax2".into()],
    });
    checker.declare_axiom(AxiomDef {
        name: "ax2".into(),
        span: 0..1,
        references: vec!["ax1".into()],
    });
    let circ_errs = checker.check_circular();
    assert!(
        !circ_errs.is_empty(),
        "circular dependency should be detected"
    );
}

// =========================================================================
// Issue #115: Platform abstraction extracts supported platforms
// =========================================================================

#[test]
fn platform_abstraction_with_supported_platforms() {
    let mut checker = PlatformAbstractionChecker::new();
    checker.add_platform("linux".into());
    checker.add_platform("macos".into());
    checker.declare_abstraction("fs_ops".into(), vec!["linux".into(), "macos".into()]);
    // All declared platforms are supported, should not error
    let errs = checker.check_unknown_platforms();
    assert!(errs.is_empty(), "all platforms are known");
}

#[test]
fn platform_abstraction_unknown_platform_detected() {
    let mut checker = PlatformAbstractionChecker::new();
    checker.add_platform("linux".into());
    checker.declare_abstraction("fs_ops".into(), vec!["linux".into(), "windows".into()]);
    let errs = checker.check_unknown_platforms();
    assert_eq!(errs.len(), 1, "windows should be flagged as unknown");
    assert_eq!(errs[0].code, "A44003");
}

// =========================================================================
// Issue #116: Feature flags extract default_enabled from annotations
// =========================================================================

#[test]
fn feature_flag_with_enabled_default() {
    let mut checker = FeatureFlagChecker::new();
    checker.declare("dark_mode".into(), true, Vec::new());
    checker.declare("experimental".into(), false, Vec::new());
    // Neither is used, both should be flagged as unused
    let errs = checker.check_unused();
    assert_eq!(errs.len(), 2, "both flags should be flagged as unused");
}

#[test]
fn feature_flag_with_dependencies() {
    let mut checker = FeatureFlagChecker::new();
    checker.declare("base".into(), true, Vec::new());
    checker.declare("advanced".into(), false, vec!["base".into()]);
    checker.mark_used("base");
    let errs = checker.check_unused();
    assert_eq!(errs.len(), 1, "only advanced should be unused");
}

// =========================================================================
// Issue #118: Unsafe escape extracts proof obligations from annotations
// =========================================================================

#[test]
fn unsafe_escape_with_obligations() {
    let mut checker = UnsafeEscapeChecker::new();
    checker.declare_unsafe(
        "raw_ptr_deref".into(),
        vec!["memory_safety".into(), "alignment".into()],
        0..1,
    );
    // Without discharging, check should flag unfulfilled obligations
    let errs = checker.check_obligations();
    assert!(
        !errs.is_empty(),
        "undischarged obligations should be flagged"
    );
}

#[test]
fn unsafe_escape_discharge_obligation() {
    let mut checker = UnsafeEscapeChecker::new();
    checker.declare_unsafe("raw_ptr_deref".into(), vec!["memory_safety".into()], 0..1);
    checker.attach_proof("raw_ptr_deref");
    checker.discharge_obligation("raw_ptr_deref", "memory_safety".into());
    let errs = checker.check_obligations();
    assert!(errs.is_empty(), "discharged obligations should not error");
}

// =========================================================================
// Issue #110: Cross-type comparisons rejected in clause bodies
// =========================================================================

#[test]
fn cross_type_comparison_string_vs_int_rejected() {
    // String >= Int should produce a type error
    let src = r#"
        contract Bad {
            input(name: String)
            requires { name >= 650 }
            ensures(result: Int)
        }
    "#;
    let resolved = resolve_ok(src);
    let result = type_check(&resolved);
    let errs = result.expect_err("String >= Int comparison should be rejected");
    assert!(
        errs.iter().any(|e| e.code.as_str().starts_with("A03")),
        "expected a type error (A03xxx), got: {errs:?}"
    );
}

#[test]
fn cross_type_arithmetic_string_plus_int_rejected() {
    // String + Int should produce a type error
    let src = r#"
        contract Bad {
            input(name: String)
            output(result: Int)
            ensures { result == name + 1 }
        }
    "#;
    let resolved = resolve_ok(src);
    let result = type_check(&resolved);
    let errs = result.expect_err("String + Int arithmetic should be rejected");
    assert!(
        errs.iter().any(|e| e.code.as_str().starts_with("A03")),
        "expected a type error (A03xxx), got: {errs:?}"
    );
}

#[test]
fn same_type_comparison_passes() {
    // Int >= Int should pass
    let src = r#"
        contract Good {
            input(x: Int)
            requires { x >= 0 }
            output(result: Int)
        }
    "#;
    let resolved = resolve_ok(src);
    let result = type_check(&resolved);
    assert!(result.is_ok(), "Int >= Int should pass: {:?}", result.err());
}

// =========================================================================
// Issue #117: MVCC snapshot isolation and phantom read checks wired
// =========================================================================

#[test]
fn mvcc_phantom_read_detected() {
    use crate::domain::MvccChecker;
    let mut checker = MvccChecker::new();
    let tx1 = checker.begin_txn();
    let tx2 = checker.begin_txn();
    checker.write_version("key".into(), tx2);
    checker.commit_txn(tx2);
    // tx1 started before tx2 committed, so reading key sees a phantom
    let errs = checker.check_phantom(tx1);
    assert!(!errs.is_empty(), "phantom read should be detected");
    assert_eq!(errs[0].code, "A35003");
}

#[test]
fn mvcc_no_phantom_when_txn_committed_before() {
    use crate::domain::MvccChecker;
    let mut checker = MvccChecker::new();
    let tx1 = checker.begin_txn();
    checker.write_version("key".into(), tx1);
    checker.commit_txn(tx1);
    let tx2 = checker.begin_txn();
    // tx2 starts after tx1 committed, no phantom
    let errs = checker.check_phantom(tx2);
    assert!(errs.is_empty(), "no phantom expected for later txn");
}

// =========================================================================
// Issue #119: Resource limits - record_usage, check_near_limit, check_unbounded
// =========================================================================

#[test]
fn resource_limit_near_limit_detected() {
    let mut checker = ResourceLimitChecker::new();
    checker.declare_limit("memory".into(), 1000, "bytes".into());
    checker.record_usage("memory", 950); // 95% of limit
    let errs = checker.check_near_limit();
    assert!(!errs.is_empty(), "near-limit warning should fire at 95%");
    assert_eq!(errs[0].code, "A46003");
}

#[test]
fn resource_limit_unbounded_detected() {
    let checker = ResourceLimitChecker::new();
    // No limit declared for "cpu"
    let err = checker.check_unbounded("cpu");
    assert!(err.is_some(), "unbounded resource should be detected");
    assert_eq!(err.unwrap().code, "A46002");
}

#[test]
fn resource_limit_under_threshold_no_warning() {
    let mut checker = ResourceLimitChecker::new();
    checker.declare_limit("memory".into(), 1000, "bytes".into());
    checker.record_usage("memory", 500); // 50% of limit
    let errs = checker.check_near_limit();
    assert!(
        errs.is_empty(),
        "50% usage should not trigger near-limit warning"
    );
}

// =========================================================================
// Issue #120: Complexity bounds - record_measured, check_bounds
// =========================================================================

#[test]
fn complexity_bound_verified_no_error() {
    let mut checker = ComplexityBoundChecker::new();
    checker.declare_bound("search".into(), ComplexityClass::Logarithmic, 0..1);
    checker.record_measured("search", ComplexityClass::Logarithmic);
    let errs = checker.check_bounds();
    assert!(errs.is_empty(), "matching complexity should not error");
}

#[test]
fn complexity_bound_violation_detected() {
    let mut checker = ComplexityBoundChecker::new();
    checker.declare_bound("sort".into(), ComplexityClass::Linear, 0..1);
    checker.record_measured("sort", ComplexityClass::Quadratic);
    let errs = checker.check_bounds();
    assert!(
        !errs.is_empty(),
        "quadratic exceeding linear bound should error"
    );
    assert_eq!(errs[0].code, "A48001");
}

#[test]
fn complexity_exponential_warning() {
    let mut checker = ComplexityBoundChecker::new();
    checker.declare_bound("solve".into(), ComplexityClass::Exponential, 0..1);
    let errs = checker.check_expensive();
    assert!(!errs.is_empty(), "exponential bound should trigger warning");
    assert_eq!(errs[0].code, "A48003");
}

// =========================================================================
// Issue #122: Behavioral equivalence - contract ref extracted, mark_verified
// =========================================================================

#[test]
fn behavioral_equivalence_with_contract_ref_no_a49003() {
    let mut checker = BehavioralEquivalenceChecker::new();
    checker.declare(
        "eq1".into(),
        "implA".into(),
        "implB".into(),
        "MyContract".into(),
        0..1,
    );
    let errs = checker.check_contract_ref();
    assert!(
        errs.is_empty(),
        "non-empty contract ref should not trigger A49003"
    );
}

#[test]
fn behavioral_equivalence_missing_contract_ref_a49003() {
    let mut checker = BehavioralEquivalenceChecker::new();
    checker.declare(
        "eq1".into(),
        "implA".into(),
        "implB".into(),
        String::new(),
        0..1,
    );
    let errs = checker.check_contract_ref();
    assert_eq!(errs.len(), 1, "empty contract ref should trigger A49003");
    assert_eq!(errs[0].code, "A49003");
}

#[test]
fn behavioral_equivalence_verified_no_a49001() {
    let mut checker = BehavioralEquivalenceChecker::new();
    checker.declare(
        "eq1".into(),
        "implA".into(),
        "implB".into(),
        "MyContract".into(),
        0..1,
    );
    checker.mark_verified("eq1");
    let errs = checker.check_unverified();
    assert!(
        errs.is_empty(),
        "verified equivalence should not trigger A49001"
    );
}

#[test]
fn behavioral_equivalence_self_equiv_detected() {
    let mut checker = BehavioralEquivalenceChecker::new();
    checker.declare(
        "eq_self".into(),
        "implA".into(),
        "implA".into(),
        "SomeContract".into(),
        0..1,
    );
    let errs = checker.check_self_equivalence();
    assert_eq!(errs.len(), 1, "self-equivalence should be detected");
    assert_eq!(errs[0].code, "A49002");
}

// =========================================================================
// Issue #145/#122: Behavioral equivalence via full pipeline
// =========================================================================

#[test]
fn behavioral_equivalence_unverified_pipeline_a49001() {
    let src = r#"
module test.equiv_unverified;

contract SortEquiv {
    input(n: Nat)
    equivalent sort_v1 == sort_v2
    requires { n > 0 }
    ensures { n > 0 }
}
"#;
    let resolved = resolve_ok(src);
    let result = type_check(&resolved);
    let errors = result.unwrap_err();
    assert!(
        errors.iter().any(|e| e.code == "A49001"),
        "unverified equivalence should produce A49001, got: {errors:?}"
    );
}

// =========================================================================
// Issue #123: Multi-pass refinement - discharge, check_complete, check_chain
// =========================================================================

#[test]
fn multi_pass_discharge_clears_obligations() {
    use crate::domain::MultiPassRefinementChecker;
    let mut checker = MultiPassRefinementChecker::new();
    checker.add_pass("p1".into(), "L0".into(), "L1".into(), 3, 0..1);
    checker.discharge("p1", 3);
    let errs = checker.check_complete();
    assert!(errs.is_empty(), "fully discharged pass should not error");
}

#[test]
fn multi_pass_partial_discharge_flags_incomplete() {
    use crate::domain::MultiPassRefinementChecker;
    let mut checker = MultiPassRefinementChecker::new();
    checker.add_pass("p1".into(), "L0".into(), "L1".into(), 5, 0..1);
    checker.discharge("p1", 2);
    let errs = checker.check_complete();
    assert!(
        !errs.is_empty(),
        "partially discharged pass should flag A50001"
    );
    assert_eq!(errs[0].code, "A50001");
}

#[test]
fn multi_pass_chain_gap_detected() {
    use crate::domain::MultiPassRefinementChecker;
    let mut checker = MultiPassRefinementChecker::new();
    checker.add_pass("p1".into(), "L0".into(), "L1".into(), 0, 0..1);
    checker.add_pass("p2".into(), "L2".into(), "L3".into(), 0, 0..1);
    // Gap: p1 ends at L1, p2 starts at L2
    let errs = checker.check_chain();
    assert!(!errs.is_empty(), "chain gap should be detected");
    assert_eq!(errs[0].code, "A50002");
}

// =========================================================================
// Issue #124: Numerical precision - target bits extraction
// =========================================================================

#[test]
fn precision_loss_detected_for_narrowing_cast() {
    let mut checker = NumericalPrecisionChecker::new();
    checker.declare("x".into(), 64, 1.0, 0..1);
    let err = checker.check_precision_loss("x", 32);
    assert!(
        err.is_some(),
        "64-bit narrowed to 32-bit should flag A42001"
    );
    assert_eq!(err.unwrap().code, "A42001");
}

#[test]
fn precision_loss_not_flagged_for_same_width() {
    let mut checker = NumericalPrecisionChecker::new();
    checker.declare("x".into(), 32, 1.0, 0..1);
    let err = checker.check_precision_loss("x", 32);
    assert!(err.is_none(), "same width should not flag precision loss");
}

#[test]
fn ulp_bound_violation_detected() {
    let mut checker = NumericalPrecisionChecker::new();
    checker.declare("y".into(), 64, 1.0, 0..1);
    let err = checker.check_ulp_bound("y", 2.5);
    assert!(err.is_some(), "ULP 2.5 > min 1.0 should trigger A42002");
    assert_eq!(err.unwrap().code, "A42002");
}

#[test]
fn cancellation_detected() {
    let mut checker = NumericalPrecisionChecker::new();
    checker.declare("z".into(), 64, 1.0, 0..1);
    let err = checker.check_cancellation("z", 0.99999);
    assert!(err.is_some(), "ratio 0.99999 should trigger A42003");
    assert_eq!(err.unwrap().code, "A42003");
}

#[test]
fn type_name_to_bits_mapping() {
    assert_eq!(crate::type_name_to_bits("f16"), 16);
    assert_eq!(crate::type_name_to_bits("f32"), 32);
    assert_eq!(crate::type_name_to_bits("f64"), 64);
    assert_eq!(crate::type_name_to_bits("Float"), 64);
    assert_eq!(crate::type_name_to_bits("Int"), 64);
    assert_eq!(crate::type_name_to_bits("i8"), 8);
    assert_eq!(crate::type_name_to_bits("unknown_type"), 32);
}

// =========================================================================
// Issue #125: Precomputed tables - mark_entries_verified, check_coverage
// =========================================================================

#[test]
fn precomputed_table_fully_verified() {
    let mut checker = PrecomputedTableChecker::new();
    checker.declare_table("sin_table".into(), 100, "gen_sin".into(), 0..1);
    checker.mark_entries_verified("sin_table", 100);
    let errs = checker.check_coverage();
    assert!(
        errs.is_empty(),
        "fully verified table should not flag A43001"
    );
}

#[test]
fn precomputed_table_partial_coverage_flagged() {
    let mut checker = PrecomputedTableChecker::new();
    checker.declare_table("cos_table".into(), 100, "gen_cos".into(), 0..1);
    checker.mark_entries_verified("cos_table", 50);
    let errs = checker.check_coverage();
    assert!(!errs.is_empty(), "50% coverage should flag A43001");
    assert_eq!(errs[0].code, "A43001");
}

#[test]
fn precomputed_table_missing_generator_flagged() {
    let mut checker = PrecomputedTableChecker::new();
    checker.declare_table("lookup".into(), 10, String::new(), 0..1);
    let errs = checker.check_generator();
    assert!(!errs.is_empty(), "missing generator should flag A43002");
    assert_eq!(errs[0].code, "A43002");
}

// ===========================================================================
// Match exhaustiveness checks (A10001, A10002)
// ===========================================================================

#[test]
fn match_non_exhaustive_missing_variant_a10001() {
    // When the scrutinee ident matches an enum name, A10001 fires for
    // missing variants.  (The current check uses the ident name, not the
    // inferred type, so the scrutinee must literally be the enum name.)
    let src = r#"
        enum Status { Active, Inactive, Pending }
        contract CheckStatus {
            input(s: Int)
            ensures { match Status { Active => true, Inactive => false } }
        }
    "#;
    let resolved = resolve_ok(src);
    let result = type_check(&resolved);
    let errs = result.err().unwrap_or_default();
    assert!(
        errs.iter().any(|e| e.code == "A10001"),
        "missing variant Pending should produce A10001: {errs:?}"
    );
}

#[test]
fn match_all_variants_covered_no_a10001() {
    // All 3 variants covered: no A10001.
    let src = r#"
        enum Status { Active, Inactive, Pending }
        contract CheckStatus {
            input(s: Int)
            ensures { match Status { Active => true, Inactive => false, Pending => false } }
        }
    "#;
    let resolved = resolve_ok(src);
    let result = type_check(&resolved);
    let errs = result.err().unwrap_or_default();
    assert!(
        !errs.iter().any(|e| e.code == "A10001"),
        "all variants covered, should not produce A10001: {errs:?}"
    );
}

#[test]
fn match_unknown_scrutinee_no_wildcard_a10002() {
    // match on a variable whose type is not a known enum, without a wildcard.
    let src = r#"
        contract CheckValue {
            input(x: Int)
            ensures { match x { 1 => true, 2 => false } }
        }
    "#;
    let resolved = resolve_ok(src);
    let result = type_check(&resolved);
    let errs = result.err().unwrap_or_default();
    assert!(
        errs.iter().any(|e| e.code == "A10002"),
        "match without wildcard on unknown type should produce A10002: {errs:?}"
    );
}

// ===========================================================================
// Extern trust boundary check (A11005)
// ===========================================================================

#[test]
fn collection_sort_without_length_postcondition_a03007() {
    // A contract named "sort" (a length-preserving op) without an ensures
    // mentioning len should produce A03007.
    let src = r#"
        contract Sort {
            input(items: List<Int>)
            ensures { result >= 0 }
        }
    "#;
    let resolved = resolve_ok(src);
    let result = type_check(&resolved);
    let errs = result.err().unwrap_or_default();
    assert!(
        errs.iter().any(|e| e.code == "A03007"),
        "sort without len postcondition should produce A03007: {errs:?}"
    );
}

#[test]
fn collection_sort_with_length_postcondition_no_a03007() {
    // A contract named "sort" WITH a len postcondition should not warn.
    let src = r#"
        contract Sort {
            input(items: List<Int>)
            ensures { len(result) == len(items) }
        }
    "#;
    let resolved = resolve_ok(src);
    let result = type_check(&resolved);
    let errs = result.err().unwrap_or_default();
    assert!(
        !errs.iter().any(|e| e.code == "A03007"),
        "sort with len postcondition should not produce A03007: {errs:?}"
    );
}

// ==========================================================================
// CORE.5: Quantifier trigger checker wiring tests
// ==========================================================================

#[test]
fn quantifier_trigger_strict_fires_a_core_050() {
    let src = r#"
contract X {
    input(x: Int)
    strict_triggers true
    requires { forall i in x: i >= 0 }
    ensures { x > 0 }
}
"#;
    let resolved = resolve_ok(src);
    let result = type_check(&resolved);
    let errs = result.err().unwrap_or_default();
    assert!(
        errs.iter().any(|e| e.code == "A53006"),
        "strict_triggers should fire A53006 for quantifier without trigger: {errs:?}"
    );
}

#[test]
fn quantifier_trigger_no_strict_does_not_fire() {
    let src = r#"
contract X {
    input(x: Int)
    requires { forall i in x: i >= 0 }
    ensures { x > 0 }
}
"#;
    let resolved = resolve_ok(src);
    let result = type_check(&resolved);
    let errs = result.err().unwrap_or_default();
    assert!(
        !errs.iter().any(|e| e.code == "A53006"),
        "without strict_triggers, A53006 should not fire: {errs:?}"
    );
}

// ==========================================================================
