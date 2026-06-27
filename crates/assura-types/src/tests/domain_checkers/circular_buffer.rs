use super::*;

// =======================================================================
// T057: CircularBufferChecker tests
// =======================================================================

#[test]
fn circ_buf_read_empty() {
    let mut checker = CircularBufferChecker::new();
    checker.declare("ring".into(), 8);
    let err = checker.check_read("ring", &(0..1));
    assert_eq!(err.unwrap().code, "A23003");
}

#[test]
fn circ_buf_read_nonempty() {
    let mut checker = CircularBufferChecker::new();
    checker.declare("ring".into(), 8);
    checker.push("ring");
    assert!(checker.check_read("ring", &(0..1)).is_none());
}

#[test]
fn circ_buf_index_out_of_bounds() {
    let mut checker = CircularBufferChecker::new();
    checker.declare("ring".into(), 4);
    let err = checker.check_index("ring", 5, &(0..1));
    assert_eq!(err.unwrap().code, "A23001");
}

#[test]
fn circ_buf_index_ok() {
    let mut checker = CircularBufferChecker::new();
    checker.declare("ring".into(), 4);
    assert!(checker.check_index("ring", 3, &(0..1)).is_none());
}

#[test]
fn circ_buf_zero_capacity() {
    let mut checker = CircularBufferChecker::new();
    checker.declare("ring".into(), 0);
    let err = checker.check_physical_wrap("ring", 0, &(0..1));
    assert_eq!(err.unwrap().code, "A23002");
}

#[test]
fn circ_buf_push_pop() {
    let mut checker = CircularBufferChecker::new();
    checker.declare("ring".into(), 2);
    checker.push("ring");
    checker.push("ring");
    // Full, push should not increase count
    checker.push("ring");
    let info = checker.buffers.get("ring").unwrap();
    assert_eq!(info.count, 2);
    assert!(info.is_full());
    checker.pop("ring");
    let info = checker.buffers.get("ring").unwrap();
    assert_eq!(info.count, 1);
}

#[test]
fn circ_buf_default() {
    let checker = CircularBufferChecker::default();
    assert!(checker.check_read("x", &(0..1)).is_none());
}
