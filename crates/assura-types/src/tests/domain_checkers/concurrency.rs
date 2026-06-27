use super::*;

// =======================================================================
// T066: CallbackReentrancyChecker tests
// =======================================================================

#[test]
fn callback_reentrant_call() {
    let mut checker = CallbackReentrancyChecker::new();
    checker.mark_non_reentrant("handle_event".into(), 0..10);
    assert!(checker.enter_call("handle_event", &(0..1)).is_empty());
    // Re-entrant call
    let errors = checker.enter_call("handle_event", &(5..6));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A24001");
}

#[test]
fn callback_reentrant_allowed() {
    let mut checker = CallbackReentrancyChecker::new();
    // Not marked non-reentrant
    assert!(checker.enter_call("handle_event", &(0..1)).is_empty());
    assert!(checker.enter_call("handle_event", &(5..6)).is_empty());
}

#[test]
fn callback_max_depth() {
    let mut checker = CallbackReentrancyChecker::new().with_max_depth(2);
    assert!(checker.enter_call("a", &(0..1)).is_empty());
    assert!(checker.enter_call("b", &(0..1)).is_empty());
    let errors = checker.enter_call("c", &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A24003");
}

#[test]
fn callback_register_in_context() {
    let mut checker = CallbackReentrancyChecker::new();
    checker.mark_non_reentrant("handler".into(), 0..10);
    assert!(checker.enter_call("handler", &(0..1)).is_empty());
    let err = checker.check_register_callback("handler", &(5..6));
    assert_eq!(err.unwrap().code, "A24002");
}

#[test]
fn callback_exit_resets() {
    let mut checker = CallbackReentrancyChecker::new();
    checker.mark_non_reentrant("f".into(), 0..10);
    assert!(checker.enter_call("f", &(0..1)).is_empty());
    checker.exit_call();
    // After exit, re-entry is allowed
    assert!(checker.enter_call("f", &(5..6)).is_empty());
}

#[test]
fn callback_depth_tracking() {
    let mut checker = CallbackReentrancyChecker::new();
    assert_eq!(checker.current_depth(), 0);
    checker.enter_call("a", &(0..1));
    assert_eq!(checker.current_depth(), 1);
    checker.enter_call("b", &(0..1));
    assert_eq!(checker.current_depth(), 2);
    checker.exit_call();
    assert_eq!(checker.current_depth(), 1);
}

#[test]
fn callback_default() {
    let checker = CallbackReentrancyChecker::default();
    assert_eq!(checker.current_depth(), 0);
}

// =======================================================================
// T069: TemporalDeadlineChecker tests
// =======================================================================

#[test]
fn deadline_operation_exceeds() {
    let mut checker = TemporalDeadlineChecker::new();
    checker.register_bound("heavy_compute".into(), 500);
    assert!(
        checker
            .enter_deadline("fast".into(), 100, &(0..1))
            .is_none()
    );
    let err = checker.check_operation("heavy_compute", &(5..6));
    assert_eq!(err.unwrap().code, "A25001");
}

#[test]
fn deadline_operation_ok() {
    let mut checker = TemporalDeadlineChecker::new();
    checker.register_bound("quick".into(), 10);
    assert!(
        checker
            .enter_deadline("normal".into(), 100, &(0..1))
            .is_none()
    );
    assert!(checker.check_operation("quick", &(5..6)).is_none());
}

#[test]
fn deadline_unbounded_operation() {
    let mut checker = TemporalDeadlineChecker::new();
    assert!(
        checker
            .enter_deadline("strict".into(), 50, &(0..1))
            .is_none()
    );
    let err = checker.check_operation("unknown_op", &(5..6));
    assert_eq!(err.unwrap().code, "A25003");
}

#[test]
fn deadline_nested_violation() {
    let mut checker = TemporalDeadlineChecker::new();
    assert!(
        checker
            .enter_deadline("outer".into(), 100, &(0..1))
            .is_none()
    );
    let err = checker.enter_deadline("inner".into(), 200, &(5..6));
    assert_eq!(err.unwrap().code, "A25002");
}

#[test]
fn deadline_nested_ok() {
    let mut checker = TemporalDeadlineChecker::new();
    assert!(
        checker
            .enter_deadline("outer".into(), 100, &(0..1))
            .is_none()
    );
    assert!(
        checker
            .enter_deadline("inner".into(), 50, &(5..6))
            .is_none()
    );
}

#[test]
fn deadline_no_context_ok() {
    let checker = TemporalDeadlineChecker::new();
    // No deadline context, any operation is fine
    assert!(checker.check_operation("anything", &(0..1)).is_none());
}

#[test]
fn deadline_current() {
    let mut checker = TemporalDeadlineChecker::new();
    assert!(checker.current_deadline().is_none());
    checker.enter_deadline("d".into(), 42, &(0..1));
    assert_eq!(checker.current_deadline(), Some(("d", 42)));
    checker.exit_deadline();
    assert!(checker.current_deadline().is_none());
}

#[test]
fn deadline_default() {
    let checker = TemporalDeadlineChecker::default();
    assert!(checker.current_deadline().is_none());
}
