//! Tests for domain checkers.

use super::*;

// -----------------------------------------------------------------------
// AllocatorChecker
// -----------------------------------------------------------------------

#[test]
fn alloc_unpaired_detected() {
    let mut ac = AllocatorChecker::new();
    ac.record_alloc("buf".into(), None, 0..10);
    let errors = ac.check_unpaired();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A22001");
}

#[test]
fn alloc_paired_ok() {
    let mut ac = AllocatorChecker::new();
    ac.record_alloc("buf".into(), None, 0..10);
    assert!(ac.record_free("buf", 10..20).is_none());
    assert!(ac.check_unpaired().is_empty());
}

#[test]
fn alloc_double_free() {
    let mut ac = AllocatorChecker::new();
    ac.record_alloc("buf".into(), None, 0..10);
    assert!(ac.record_free("buf", 10..20).is_none());
    let err = ac.record_free("buf", 20..30).unwrap();
    assert_eq!(err.code, "A22002");
}

#[test]
fn alloc_arena_use_after_drop() {
    let mut ac = AllocatorChecker::new();
    ac.declare_arena("pool".into());
    ac.record_alloc("buf".into(), Some("pool".into()), 0..10);
    ac.drop_arena("pool", 10..20);
    let err = ac.check_arena_use("buf", &(20..30)).unwrap();
    assert_eq!(err.code, "A22004");
}

#[test]
fn alloc_arena_no_error_before_drop() {
    let mut ac = AllocatorChecker::new();
    ac.declare_arena("pool".into());
    ac.record_alloc("buf".into(), Some("pool".into()), 0..10);
    assert!(ac.check_arena_use("buf", &(5..15)).is_none());
}

#[test]
fn alloc_arena_skips_unpaired() {
    let mut ac = AllocatorChecker::new();
    ac.declare_arena("pool".into());
    ac.record_alloc("buf".into(), Some("pool".into()), 0..10);
    // Arena allocs don't need explicit free
    assert!(ac.check_unpaired().is_empty());
}

#[test]
fn alloc_unbounded_detected() {
    let mut ac = AllocatorChecker::new();
    ac.record_alloc("buf".into(), None, 0..10);
    let errors = ac.check_unbounded();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A22003");
}

#[test]
fn alloc_bounded_ok() {
    let mut ac = AllocatorChecker::new();
    ac.record_alloc("buf".into(), None, 0..10);
    ac.mark_bounded("buf");
    assert!(ac.check_unbounded().is_empty());
}

// -----------------------------------------------------------------------
// CircularBufferChecker
// -----------------------------------------------------------------------

#[test]
fn circ_buf_basic() {
    let mut cb = CircularBufferChecker::new();
    cb.declare("ring".into(), 16);
    let err = cb.check_index("ring", 15, &(10..20));
    assert!(err.is_none());
}

#[test]
fn circ_buf_index_exceeds_capacity() {
    let mut cb = CircularBufferChecker::new();
    cb.declare("ring".into(), 16);
    let err = cb.check_index("ring", 20, &(10..20));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A23001");
}

#[test]
fn circ_buf_empty_read() {
    let mut cb = CircularBufferChecker::new();
    cb.declare("ring".into(), 16);
    let err = cb.check_read("ring", &(10..20));
    assert!(err.is_some());
    assert_eq!(err.unwrap().code, "A23003");
}

// -----------------------------------------------------------------------
// PlatformAbstractionChecker
// -----------------------------------------------------------------------

#[test]
fn platform_missing_coverage() {
    let mut pac = PlatformAbstractionChecker::new();
    pac.add_platform("linux".into());
    pac.add_platform("windows".into());
    pac.declare_abstraction("fs_ops".into(), vec!["linux".into()]);
    let errors = pac.check_coverage();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A44001");
    assert!(errors[0].message.contains("windows"));
}

#[test]
fn platform_full_coverage_ok() {
    let mut pac = PlatformAbstractionChecker::new();
    pac.add_platform("linux".into());
    pac.declare_abstraction("fs_ops".into(), vec!["linux".into()]);
    assert!(pac.check_coverage().is_empty());
}

#[test]
fn platform_direct_use_warned() {
    let mut pac = PlatformAbstractionChecker::new();
    pac.add_platform("linux".into());
    let err = pac.check_direct_platform_use("linux").unwrap();
    assert_eq!(err.code, "A44002");
}

#[test]
fn platform_unknown_reference() {
    let mut pac = PlatformAbstractionChecker::new();
    pac.add_platform("linux".into());
    pac.declare_abstraction("net".into(), vec!["linux".into(), "freebsd".into()]);
    let errors = pac.check_unknown_platforms();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A44003");
    assert!(errors[0].message.contains("freebsd"));
}

// -----------------------------------------------------------------------
// FeatureFlagChecker
// -----------------------------------------------------------------------

#[test]
fn feature_flag_unused() {
    let mut ffc = FeatureFlagChecker::new();
    ffc.declare("experimental".into(), false, vec![]);
    let errors = ffc.check_unused();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A45001");
}

#[test]
fn feature_flag_used_ok() {
    let mut ffc = FeatureFlagChecker::new();
    ffc.declare("experimental".into(), false, vec![]);
    ffc.mark_used("experimental");
    assert!(ffc.check_unused().is_empty());
}

#[test]
fn feature_flag_conflicts() {
    let mut ffc = FeatureFlagChecker::new();
    ffc.declare("debug".into(), true, vec!["release".into()]);
    ffc.declare("release".into(), true, vec!["debug".into()]);
    let errors = ffc.check_conflicts();
    assert!(!errors.is_empty());
    assert_eq!(errors[0].code, "A45002");
}

#[test]
fn feature_flag_no_conflict_when_disabled() {
    let mut ffc = FeatureFlagChecker::new();
    ffc.declare("debug".into(), true, vec!["release".into()]);
    ffc.declare("release".into(), false, vec!["debug".into()]);
    assert!(ffc.check_conflicts().is_empty());
}

#[test]
fn feature_flag_undeclared() {
    let ffc = FeatureFlagChecker::new();
    let err = ffc.check_undeclared("unknown").unwrap();
    assert_eq!(err.code, "A45003");
}

// -----------------------------------------------------------------------
// ResourceLimitChecker
// -----------------------------------------------------------------------

#[test]
fn resource_limit_exceeded() {
    let mut rlc = ResourceLimitChecker::new();
    rlc.declare_limit("memory".into(), 1024, "bytes".into());
    rlc.record_usage("memory", 2000);
    let errors = rlc.check_limits();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A46001");
}

#[test]
fn resource_limit_ok() {
    let mut rlc = ResourceLimitChecker::new();
    rlc.declare_limit("memory".into(), 1024, "bytes".into());
    rlc.record_usage("memory", 500);
    assert!(rlc.check_limits().is_empty());
}

#[test]
fn resource_near_limit_warned() {
    let mut rlc = ResourceLimitChecker::new();
    rlc.declare_limit("cpu".into(), 100, "percent".into());
    rlc.record_usage("cpu", 95);
    let warnings = rlc.check_near_limit();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].code, "A46003");
}

#[test]
fn resource_unbounded_usage() {
    let rlc = ResourceLimitChecker::new();
    let err = rlc.check_unbounded("unknown").unwrap();
    assert_eq!(err.code, "A46002");
}

#[test]
fn resource_release_reduces_usage() {
    let mut rlc = ResourceLimitChecker::new();
    rlc.declare_limit("mem".into(), 100, "MB".into());
    rlc.record_usage("mem", 80);
    rlc.release_usage("mem", 30);
    // After releasing 30 from 80, usage is 50 which is under the 100 limit
    assert!(rlc.check_limits().is_empty());
}

// -----------------------------------------------------------------------
// UnsafeEscapeChecker
// -----------------------------------------------------------------------

#[test]
fn unsafe_no_proof_detected() {
    let mut uec = UnsafeEscapeChecker::new();
    uec.declare_unsafe("raw_ptr".into(), vec!["ptr_valid".into()], 0..10);
    let errors = uec.check_unproven();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A47001");
}

#[test]
fn unsafe_with_proof_ok() {
    let mut uec = UnsafeEscapeChecker::new();
    uec.declare_unsafe("raw_ptr".into(), vec!["ptr_valid".into()], 0..10);
    uec.attach_proof("raw_ptr");
    uec.discharge_obligation("raw_ptr", "ptr_valid".into());
    assert!(uec.check_unproven().is_empty());
    assert!(uec.check_obligations().is_empty());
}

#[test]
fn unsafe_partial_discharge() {
    let mut uec = UnsafeEscapeChecker::new();
    uec.declare_unsafe(
        "raw_ptr".into(),
        vec!["ptr_valid".into(), "no_alias".into()],
        0..10,
    );
    uec.attach_proof("raw_ptr");
    uec.discharge_obligation("raw_ptr", "ptr_valid".into());
    let errors = uec.check_obligations();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A47002");
}

// -----------------------------------------------------------------------
// ContractLibraryChecker
// -----------------------------------------------------------------------

#[test]
fn library_empty_exports() {
    let mut clc = ContractLibraryChecker::new();
    clc.declare_library("math".into(), "2.0.0".into());
    let errors = clc.check_empty_exports();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A55001");
}

#[test]
fn library_with_export_ok() {
    let mut clc = ContractLibraryChecker::new();
    clc.declare_library("math".into(), "1.0.0".into());
    clc.add_export("math", "Arithmetic".into());
    assert!(clc.check_empty_exports().is_empty());
}

#[test]
fn library_self_dependency() {
    let mut clc = ContractLibraryChecker::new();
    clc.declare_library("core".into(), "1.0.0".into());
    clc.add_dependency(
        "core",
        LibraryDep {
            name: "core".into(),
            version_req: "1.0.0".into(),
        },
    );
    let errors = clc.check_circular_deps();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A55002");
}

#[test]
fn library_duplicates() {
    let mut clc = ContractLibraryChecker::new();
    clc.declare_library("math".into(), "1.0.0".into());
    clc.declare_library("math".into(), "2.0.0".into());
    let errors = clc.check_duplicates();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A55003");
}

// -----------------------------------------------------------------------
// ContractCompositionChecker
// -----------------------------------------------------------------------

#[test]
fn composition_extends_unknown() {
    let mut ccc = ContractCompositionChecker::new();
    ccc.declare("MySorter".into(), vec!["Sortable".into()], 2);
    let errors = ccc.check_extends();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A54001");
}

#[test]
fn composition_extends_ok() {
    let mut ccc = ContractCompositionChecker::new();
    ccc.declare("Sortable".into(), vec![], 3);
    ccc.declare("MySorter".into(), vec!["Sortable".into()], 2);
    assert!(ccc.check_extends().is_empty());
}

#[test]
fn composition_circular() {
    let mut ccc = ContractCompositionChecker::new();
    ccc.declare("A".into(), vec!["B".into()], 1);
    ccc.declare("B".into(), vec!["A".into()], 1);
    let errors = ccc.check_circular();
    assert!(!errors.is_empty());
    assert_eq!(errors[0].code, "A54002");
}

// -----------------------------------------------------------------------
// StorageFailureChecker
// -----------------------------------------------------------------------

#[test]
fn storage_unhandled_failure() {
    let mut sfc = StorageFailureChecker::new();
    sfc.declare_failure_mode(FailureMode::PartialWrite);
    let errors = sfc.check_unhandled();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A38001");
}

#[test]
fn storage_handled_ok() {
    let mut sfc = StorageFailureChecker::new();
    sfc.declare_failure_mode(FailureMode::DiskFull);
    sfc.mark_handled("disk_full");
    assert!(sfc.check_unhandled().is_empty());
}

#[test]
fn storage_spurious_handler() {
    let mut sfc = StorageFailureChecker::new();
    sfc.mark_handled("nonexistent");
    let errors = sfc.check_spurious_handlers();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A38002");
}
