use super::*;

// =======================================================================
// T086: CrashRecoveryChecker tests
// =======================================================================

#[test]
fn crash_recovery_write_ahead_violation() {
    let mut cr = CrashRecoveryChecker::new();
    cr.begin_write("txn1".into());
    cr.write_data("txn1");
    let errs = cr.check_write_ahead();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A33001");
}

#[test]
fn crash_recovery_write_ahead_ok() {
    let mut cr = CrashRecoveryChecker::new();
    cr.begin_write("txn1".into());
    cr.write_wal("txn1");
    cr.write_data("txn1");
    assert!(cr.check_write_ahead().is_empty());
}

#[test]
fn crash_recovery_commit_without_fsync() {
    let mut cr = CrashRecoveryChecker::new();
    cr.begin_write("txn1".into());
    cr.commit("txn1");
    let errs = cr.check_commit_durability();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A33002");
}

#[test]
fn crash_recovery_fsync_before_data() {
    let mut cr = CrashRecoveryChecker::new();
    cr.begin_write("txn1".into());
    cr.write_wal("txn1");
    cr.fsync("txn1");
    let errs = cr.check_ordering();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A33003");
}

#[test]
fn crash_recovery_full_sequence_ok() {
    let mut cr = CrashRecoveryChecker::new();
    cr.begin_write("txn1".into());
    cr.write_wal("txn1");
    cr.write_data("txn1");
    cr.fsync("txn1");
    cr.commit("txn1");
    assert!(cr.check_all().is_empty());
}

#[test]
fn crash_recovery_default() {
    let cr = CrashRecoveryChecker::default();
    assert!(cr.check_all().is_empty());
}

// =======================================================================
// T087: PageCacheChecker tests
// =======================================================================

#[test]
fn page_cache_evict_pinned() {
    let mut pc = PageCacheChecker::new(10);
    pc.load_page(1);
    pc.pin(1);
    let err = pc.evict(1);
    assert_eq!(err.unwrap().code, "A34001");
}

#[test]
fn page_cache_evict_dirty() {
    let mut pc = PageCacheChecker::new(10);
    pc.load_page(1);
    pc.mark_dirty(1);
    let err = pc.evict(1);
    assert_eq!(err.unwrap().code, "A34002");
}

#[test]
fn page_cache_evict_clean_ok() {
    let mut pc = PageCacheChecker::new(10);
    pc.load_page(1);
    assert!(pc.evict(1).is_none());
}

#[test]
fn page_cache_capacity_exceeded() {
    let mut pc = PageCacheChecker::new(2);
    pc.load_page(1);
    pc.load_page(2);
    pc.load_page(3);
    let errs = pc.check_capacity();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A34003");
}

#[test]
fn page_cache_flush_then_evict() {
    let mut pc = PageCacheChecker::new(10);
    pc.load_page(1);
    pc.mark_dirty(1);
    pc.flush(1);
    assert!(pc.evict(1).is_none());
}

#[test]
fn page_cache_unpin_then_evict() {
    let mut pc = PageCacheChecker::new(10);
    pc.load_page(1);
    pc.pin(1);
    pc.unpin(1);
    assert!(pc.evict(1).is_none());
}

#[test]
fn page_cache_default() {
    let pc = PageCacheChecker::default();
    assert!(pc.check_capacity().is_empty());
}

// =======================================================================
// T088: MvccChecker tests
// =======================================================================

#[test]
fn mvcc_write_conflict() {
    let mut mv = MvccChecker::new();
    let t1 = mv.begin_txn();
    let t2 = mv.begin_txn();
    mv.write_version("key1".into(), t1);
    mv.write_version("key1".into(), t2);
    let errs = mv.check_write_conflicts();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A35001");
}

#[test]
fn mvcc_no_conflict_after_commit() {
    let mut mv = MvccChecker::new();
    let t1 = mv.begin_txn();
    mv.write_version("key1".into(), t1);
    mv.commit_txn(t1);
    let t2 = mv.begin_txn();
    mv.write_version("key1".into(), t2);
    assert!(mv.check_write_conflicts().is_empty());
}

#[test]
fn mvcc_snapshot_violation() {
    let mut mv = MvccChecker::new();
    let t1 = mv.begin_txn();
    let t2 = mv.begin_txn();
    mv.write_version("key1".into(), t1);
    let err = mv.check_snapshot_read("key1", t2);
    assert_eq!(err.unwrap().code, "A35002");
}

#[test]
fn mvcc_phantom_read() {
    let mut mv = MvccChecker::new();
    let t1 = mv.begin_txn();
    let t2 = mv.begin_txn();
    mv.write_version("key1".into(), t2);
    mv.commit_txn(t2);
    let errs = mv.check_phantom(t1);
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A35003");
}

#[test]
fn mvcc_default() {
    let mv = MvccChecker::default();
    assert!(mv.check_write_conflicts().is_empty());
}

// =======================================================================
// T089: RollbackChecker tests
// =======================================================================

#[test]
fn rollback_unknown_savepoint() {
    let mut rb = RollbackChecker::new();
    let err = rb.rollback_to("sp1");
    assert_eq!(err.unwrap().code, "A36001");
}

#[test]
fn rollback_resource_leak() {
    let mut rb = RollbackChecker::new();
    rb.create_savepoint("sp1".into());
    rb.acquire_resource("lock1".into());
    rb.rollback_to("sp1");
    let errs = rb.check_resource_leak();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A36002");
}

#[test]
fn rollback_resource_released_ok() {
    let mut rb = RollbackChecker::new();
    rb.create_savepoint("sp1".into());
    rb.acquire_resource("lock1".into());
    rb.release_resource("lock1");
    rb.rollback_to("sp1");
    assert!(rb.check_resource_leak().is_empty());
}

#[test]
fn rollback_duplicate_savepoint() {
    let mut rb = RollbackChecker::new();
    rb.create_savepoint("sp1".into());
    rb.create_savepoint("sp1".into());
    let errs = rb.check_savepoint_nesting();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A36003");
}

#[test]
fn rollback_default() {
    let rb = RollbackChecker::default();
    assert!(rb.check_resource_leak().is_empty());
}

// =======================================================================
// T090: MonotonicStateChecker tests
// =======================================================================

#[test]
fn monotonic_increasing_violation() {
    let mut mc = MonotonicStateChecker::new();
    mc.declare("seq".into(), MonotonicDirection::Increasing, 10, 0..1);
    let err = mc.update("seq", 5);
    assert_eq!(err.unwrap().code, "A37001");
}

#[test]
fn monotonic_increasing_ok() {
    let mut mc = MonotonicStateChecker::new();
    mc.declare("seq".into(), MonotonicDirection::Increasing, 10, 0..1);
    assert!(mc.update("seq", 10).is_none()); // equal allowed for Increasing
    assert!(mc.update("seq", 15).is_none());
}

#[test]
fn monotonic_strictly_increasing() {
    let mut mc = MonotonicStateChecker::new();
    mc.declare(
        "ts".into(),
        MonotonicDirection::StrictlyIncreasing,
        10,
        0..1,
    );
    let err = mc.update("ts", 10); // equal not allowed
    assert_eq!(err.unwrap().code, "A37001");
}

#[test]
fn monotonic_reset_blocked() {
    let mc = MonotonicStateChecker::new();
    assert!(mc.check_reset("seq").is_none()); // not declared = no error
}

#[test]
fn monotonic_reset_declared() {
    let mut mc = MonotonicStateChecker::new();
    mc.declare("seq".into(), MonotonicDirection::Increasing, 0, 0..1);
    let err = mc.check_reset("seq");
    assert_eq!(err.unwrap().code, "A37002");
}

#[test]
fn monotonic_undeclared_access() {
    let mc = MonotonicStateChecker::new();
    let err = mc.check_access("unknown");
    assert_eq!(err.unwrap().code, "A37003");
}

#[test]
fn monotonic_current_value() {
    let mut mc = MonotonicStateChecker::new();
    mc.declare("seq".into(), MonotonicDirection::Increasing, 42, 0..1);
    assert_eq!(mc.current_value("seq"), Some(42));
    mc.update("seq", 100);
    assert_eq!(mc.current_value("seq"), Some(100));
}

#[test]
fn monotonic_default() {
    let mc = MonotonicStateChecker::default();
    mc.check_access("x").unwrap();
}

// =======================================================================
// T091: StorageFailureChecker tests
// =======================================================================

#[test]
fn storage_failure_unhandled() {
    let mut sf = StorageFailureChecker::new();
    sf.declare_failure_mode(FailureMode::PartialWrite);
    let errs = sf.check_unhandled();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A38001");
}

#[test]
fn storage_failure_handled_ok() {
    let mut sf = StorageFailureChecker::new();
    sf.declare_failure_mode(FailureMode::BitRot);
    sf.mark_handled("bit_rot");
    assert!(sf.check_unhandled().is_empty());
}

#[test]
fn storage_failure_spurious_handler() {
    let mut sf = StorageFailureChecker::new();
    sf.mark_handled("nonexistent");
    let errs = sf.check_spurious_handlers();
    assert_eq!(errs.len(), 1);
    assert_eq!(errs[0].code, "A38002");
}

#[test]
fn storage_failure_critical_coverage() {
    let mut sf = StorageFailureChecker::new();
    sf.declare_failure_mode(FailureMode::PartialWrite);
    sf.declare_failure_mode(FailureMode::TornPage);
    let errs = sf.check_critical_coverage();
    assert_eq!(errs.len(), 2);
    assert!(errs.iter().all(|e| e.code == "A38003"));
}

#[test]
fn storage_failure_default() {
    let sf = StorageFailureChecker::default();
    assert!(sf.check_critical_coverage().is_empty());
}

