use super::*;

// =======================================================================
// T097: PlatformAbstractionChecker tests
// =======================================================================

#[test]
fn platform_missing_impl() {
    let mut pa = PlatformAbstractionChecker::new();
    pa.add_platform("linux".into());
    pa.add_platform("windows".into());
    pa.declare_abstraction("fs_ops".into(), vec!["linux".into()]);
    let errs = pa.check_coverage();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A44001");
}

#[test]
fn platform_full_coverage_ok() {
    let mut pa = PlatformAbstractionChecker::new();
    pa.add_platform("linux".into());
    pa.declare_abstraction("fs_ops".into(), vec!["linux".into()]);
    assert!(pa.check_coverage().is_empty());
}

#[test]
fn platform_direct_use() {
    let mut pa = PlatformAbstractionChecker::new();
    pa.add_platform("linux".into());
    let err = pa.check_direct_platform_use("linux");
    assert_eq!(err.unwrap().code, "A44002");
}

#[test]
fn platform_unknown() {
    let mut pa = PlatformAbstractionChecker::new();
    pa.add_platform("linux".into());
    pa.declare_abstraction("net".into(), vec!["freebsd".into()]);
    let errs = pa.check_unknown_platforms();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A44003");
}

#[test]
fn platform_default() {
    let pa = PlatformAbstractionChecker::default();
    assert!(pa.check_coverage().is_empty());
}

// =======================================================================
// T098: FeatureFlagChecker tests
// =======================================================================

#[test]
fn feature_flag_unused() {
    let mut ff = FeatureFlagChecker::new();
    ff.declare("debug_mode".into(), false, vec![]);
    let errs = ff.check_unused();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A45001");
}

#[test]
fn feature_flag_used_ok() {
    let mut ff = FeatureFlagChecker::new();
    ff.declare("debug_mode".into(), false, vec![]);
    ff.mark_used("debug_mode");
    assert!(ff.check_unused().is_empty());
}

#[test]
fn feature_flag_conflict() {
    let mut ff = FeatureFlagChecker::new();
    ff.declare("a".into(), true, vec!["b".into()]);
    ff.declare("b".into(), true, vec![]);
    let errs = ff.check_conflicts();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A45002");
}

#[test]
fn feature_flag_undeclared() {
    let ff = FeatureFlagChecker::new();
    let err = ff.check_undeclared("unknown");
    assert_eq!(err.unwrap().code, "A45003");
}

#[test]
fn feature_flag_default() {
    let ff = FeatureFlagChecker::default();
    assert!(ff.check_unused().is_empty());
}

// =======================================================================
// T099: ResourceLimitChecker tests
// =======================================================================

#[test]
fn resource_limit_exceeded() {
    let mut rl = ResourceLimitChecker::new();
    rl.declare_limit("mem".into(), 1000, "bytes".into());
    rl.record_usage("mem", 1500);
    let errs = rl.check_limits();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A46001");
}

#[test]
fn resource_limit_ok() {
    let mut rl = ResourceLimitChecker::new();
    rl.declare_limit("mem".into(), 1000, "bytes".into());
    rl.record_usage("mem", 500);
    assert!(rl.check_limits().is_empty());
}

#[test]
fn resource_unbounded() {
    let rl = ResourceLimitChecker::new();
    let err = rl.check_unbounded("unknown");
    assert_eq!(err.unwrap().code, "A46002");
}

#[test]
fn resource_near_limit() {
    let mut rl = ResourceLimitChecker::new();
    rl.declare_limit("fds".into(), 100, "count".into());
    rl.record_usage("fds", 95);
    let errs = rl.check_near_limit();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A46003");
}

#[test]
fn resource_release() {
    let mut rl = ResourceLimitChecker::new();
    rl.declare_limit("mem".into(), 100, "bytes".into());
    rl.record_usage("mem", 80);
    rl.release_usage("mem", 50);
    // After releasing 50 from 80, usage is 30 which is under the 100 limit
    assert!(rl.check_limits().is_empty());
}

#[test]
fn resource_default() {
    let rl = ResourceLimitChecker::default();
    assert!(rl.check_limits().is_empty());
}
