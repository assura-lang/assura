use super::*;

// --- T065: Shared memory protocol tests ---

#[test]
fn shared_mem_read_exclusive_ok() {
    let mut checker = SharedMemChecker::new();
    checker.set_mode("buffer".into(), AccessMode::Exclusive);
    let errors = checker.check_read("buffer", &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn shared_mem_read_shared_ok() {
    let mut checker = SharedMemChecker::new();
    checker.set_mode("buffer".into(), AccessMode::SharedRead);
    let errors = checker.check_read("buffer", &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn shared_mem_read_none_a18001() {
    let mut checker = SharedMemChecker::new();
    checker.set_mode("buffer".into(), AccessMode::None);
    let errors = checker.check_read("buffer", &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A18001");
}

#[test]
fn shared_mem_write_exclusive_ok() {
    let mut checker = SharedMemChecker::new();
    checker.set_mode("buffer".into(), AccessMode::Exclusive);
    let errors = checker.check_write("buffer", &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn shared_mem_write_shared_a18002() {
    let mut checker = SharedMemChecker::new();
    checker.set_mode("buffer".into(), AccessMode::SharedRead);
    let errors = checker.check_write("buffer", &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A18002");
}

#[test]
fn shared_mem_data_race_a18003() {
    let checker = SharedMemChecker::new();
    let errors = checker.check_data_race(
        "counter",
        AccessMode::Exclusive,
        AccessMode::SharedRead,
        &(0..1),
    );
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A18003");
}

#[test]
fn shared_mem_two_readers_ok() {
    let checker = SharedMemChecker::new();
    let errors = checker.check_data_race(
        "counter",
        AccessMode::SharedRead,
        AccessMode::SharedRead,
        &(0..1),
    );
    assert!(errors.is_empty(), "two shared readers is safe");
}

#[test]
fn shared_mem_access_mode_display() {
    assert_eq!(AccessMode::Exclusive.to_string(), "exclusive");
    assert_eq!(AccessMode::SharedRead.to_string(), "shared_read");
    assert_eq!(AccessMode::None.to_string(), "none");
}
