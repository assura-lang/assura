use super::*;

// =======================================================================
// T056: AllocatorChecker tests
// =======================================================================

#[test]
fn allocator_unpaired_alloc() {
    let mut checker = AllocatorChecker::new();
    checker.record_alloc("buf".into(), None, 0..4);
    let errors = checker.check_unpaired();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A22001");
}

#[test]
fn allocator_paired_ok() {
    let mut checker = AllocatorChecker::new();
    checker.record_alloc("buf".into(), None, 0..4);
    assert!(checker.record_free("buf", 10..14).is_none());
    let errors = checker.check_unpaired();
    assert!(errors.is_empty());
}

#[test]
fn allocator_double_free() {
    let mut checker = AllocatorChecker::new();
    checker.record_alloc("buf".into(), None, 0..4);
    assert!(checker.record_free("buf", 10..14).is_none());
    let err = checker.record_free("buf", 20..24);
    assert_eq!(err.unwrap().code, "A22002");
}

#[test]
fn allocator_arena_ok() {
    let mut checker = AllocatorChecker::new();
    checker.declare_arena("arena1".into());
    checker.record_alloc("obj".into(), Some("arena1".into()), 0..4);
    // Arena-managed allocations are not required to have explicit free
    let errors = checker.check_unpaired();
    assert!(errors.is_empty());
}

#[test]
fn allocator_arena_use_after_drop() {
    let mut checker = AllocatorChecker::new();
    checker.declare_arena("arena1".into());
    checker.record_alloc("obj".into(), Some("arena1".into()), 0..4);
    checker.drop_arena("arena1", 10..14);
    let err = checker.check_arena_use("obj", &(20..24));
    assert_eq!(err.unwrap().code, "A22004");
}

#[test]
fn allocator_arena_use_before_drop_ok() {
    let mut checker = AllocatorChecker::new();
    checker.declare_arena("arena1".into());
    checker.record_alloc("obj".into(), Some("arena1".into()), 0..4);
    let err = checker.check_arena_use("obj", &(5..8));
    assert!(err.is_none());
}

#[test]
fn allocator_default() {
    let checker = AllocatorChecker::default();
    assert!(checker.check_unpaired().is_empty());
}

#[test]
fn allocator_unbounded_detected() {
    let mut checker = AllocatorChecker::new();
    checker.record_alloc("heap_buf".into(), None, 0..4);
    let errors = checker.check_unbounded();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A22003");
    assert!(errors[0].message.contains("unbounded"));
}

#[test]
fn allocator_bounded_no_error() {
    let mut checker = AllocatorChecker::new();
    checker.record_alloc("heap_buf".into(), None, 0..4);
    checker.mark_bounded("heap_buf");
    assert!(checker.check_unbounded().is_empty());
}
